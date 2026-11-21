//! Catch named CE wire identities used as raw branch keys in production Rust.
//! Field names, operator extensions, persisted SQL and independent protocol fixtures
//! can use the same text in a different role.
use halcyon_protocol::ce::{KnownQueue, LobbyState, RpcMethod};
use std::{fs, path::Path};
use syn::{
    BinOp, Expr, Lit, Pat, Token,
    parse::{Parse, ParseStream},
    visit::{self, Visit},
};

#[derive(Clone, Copy)]
enum Key {
    Method,
    Queue,
    State,
}

fn key(expr: &Expr) -> Option<Key> {
    let name = match expr {
        Expr::MethodCall(call) if call.method == "as_str" || call.method == "as_deref" => {
            return key(&call.receiver);
        }
        Expr::Reference(r) => return key(&r.expr),
        Expr::Paren(p) => return key(&p.expr),
        Expr::Field(f) => match &f.member {
            syn::Member::Named(n) => n,
            _ => return None,
        },
        Expr::Path(path) => path.path.get_ident()?,
        _ => return None,
    };
    match name.to_string().as_str() {
        "method" | "operation" => Some(Key::Method),
        "queue" | "mode" | "lobby" => Some(Key::Queue),
        "state" => Some(Key::State),
        _ => None,
    }
}

#[derive(Default)]
struct Branches(Vec<String>);

impl Branches {
    fn literal(&mut self, key: Key, lit: &Lit) {
        let Lit::Str(s) = lit else { return };
        let value = s.value();
        let known = match key {
            Key::Method => RpcMethod::from_wire(&value) != RpcMethod::Unknown,
            Key::Queue => KnownQueue::parse(&value).is_some(),
            Key::State => <LobbyState as serde::Deserialize>::deserialize(
                serde::de::value::StrDeserializer::<serde::de::value::Error>::new(&value),
            )
            .is_ok(),
        };
        if known {
            self.0.push(value);
        }
    }

    fn pattern(&mut self, key: Key, pat: &Pat) {
        match pat {
            Pat::Lit(lit) => self.literal(key, &lit.lit),
            Pat::Or(p) => p.cases.iter().for_each(|case| self.pattern(key, case)),
            Pat::Paren(p) => self.pattern(key, &p.pat),
            Pat::TupleStruct(p) if p.path.is_ident("Some") => {
                p.elems.iter().for_each(|pat| self.pattern(key, pat));
            }
            _ => {}
        }
    }
}

struct MatchInput(Expr, Pat);
impl Parse for MatchInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let subject = input.parse()?;
        input.parse::<Token![,]>()?;
        let pattern = Pat::parse_multi_with_leading_vert(input)?;
        if input.peek(Token![if]) {
            input.parse::<Token![if]>()?;
            input.parse::<Expr>()?;
        }
        input.parse::<Option<Token![,]>>()?;
        Ok(Self(subject, pattern))
    }
}

fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path()
            .segments
            .last()
            .is_some_and(|last| last.ident == "test")
            || attr.path().is_ident("cfg")
                && attr.parse_args::<syn::Ident>().is_ok_and(|id| id == "test")
    })
}

impl<'a> Visit<'a> for Branches {
    fn visit_item_fn(&mut self, f: &'a syn::ItemFn) {
        if !test_only(&f.attrs) {
            visit::visit_item_fn(self, f);
        }
    }
    fn visit_item_mod(&mut self, m: &'a syn::ItemMod) {
        if !test_only(&m.attrs) {
            visit::visit_item_mod(self, m);
        }
    }
    fn visit_expr_binary(&mut self, b: &'a syn::ExprBinary) {
        if matches!(b.op, BinOp::Eq(_) | BinOp::Ne(_)) {
            for (subject, other) in [(&*b.left, &*b.right), (&*b.right, &*b.left)] {
                if let (Some(k), Expr::Lit(lit)) = (key(subject), other) {
                    self.literal(k, &lit.lit);
                }
            }
        }
        visit::visit_expr_binary(self, b);
    }
    fn visit_expr_match(&mut self, m: &'a syn::ExprMatch) {
        if let Some(k) = key(&m.expr) {
            for arm in &m.arms {
                self.pattern(k, &arm.pat);
            }
        }
        visit::visit_expr_match(self, m);
    }
    fn visit_macro(&mut self, m: &'a syn::Macro) {
        if m.path.is_ident("matches")
            && let Ok(MatchInput(expr, pat)) = syn::parse2(m.tokens.clone())
            && let Some(k) = key(&expr)
        {
            self.pattern(k, &pat);
        }
    }
}

fn check(dir: &Path) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().unwrap() != "tests" {
                check(&path);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && path.file_name().unwrap() != "tests.rs"
        {
            let file = syn::parse_file(&fs::read_to_string(&path).unwrap()).unwrap();
            let mut found = Branches::default();
            found.visit_file(&file);
            assert!(
                found.0.is_empty(),
                "{}: use a CE enum for {:?}",
                path.display(),
                found.0
            );
        }
    }
}

#[test]
fn production_ce_branches_use_registered_names() {
    for member in fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap() {
        let dir = member.unwrap().path();
        for name in ["src", "benches", "examples"] {
            if dir.join(name).is_dir() {
                check(&dir.join(name));
            }
        }
    }
}

#[test]
fn distinguishes_branch_keys_from_fields_and_extensions() {
    let file = syn::parse_file(
        r#"
        fn check(operation: String, queue: String) {
            let _ = matches!(operation.as_str(), "guildRedeemChest" | "operatorMethod");
            let _ = "ranked" == queue;
            let _ = queue == "community_mode";
            let _ = state.get("guildInviteOnly");
            match response.state { "post_match" => (), _ => () }
        }
        #[cfg(test)] mod tests { fn golden() { match mode { "ranked" => (), _ => () } } }
    "#,
    )
    .unwrap();
    let mut found = Branches::default();
    found.visit_file(&file);
    assert_eq!(found.0, ["guildRedeemChest", "ranked", "post_match"]);
}

#[test]
fn lobby_states_keep_their_public_and_serialized_spelling() {
    const END_STATE: &str = LobbyState::PostMatch.as_str();
    assert_eq!(END_STATE, "post_match");
    for value in [
        "menus",
        "pending_auto",
        "match_pending",
        "matched_partners",
        "playing",
        "spectating",
        "post_match",
    ] {
        let state: LobbyState = serde_json::from_value(value.into()).unwrap();
        assert_eq!(state.as_str(), value);
        assert_eq!(serde_json::to_value(state).unwrap(), value);
    }
    assert!(serde_json::from_value::<LobbyState>("operator_extension".into()).is_err());
}

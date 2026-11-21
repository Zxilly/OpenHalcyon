//! Keep known CE opcodes named at raw-message boundaries. This checks syntax
//! in workspace members, including tests and examples, without expanding macros.
use halcyon_protocol::Opcode;
use std::{
    fs,
    path::{Path, PathBuf},
};
use syn::{
    BinOp, Expr, Lit, Pat, Token,
    parse::{Parse, ParseStream, Parser},
    punctuated::Punctuated,
    visit::{self, Visit},
};

fn integer(expr: &Expr) -> Option<u16> {
    match expr {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Int(int) => int.base10_parse().ok(),
            _ => None,
        },
        Expr::Paren(p) => integer(&p.expr),
        Expr::Group(g) => integer(&g.expr),
        _ => None,
    }
}

fn opcode(expr: &Expr) -> bool {
    match expr {
        Expr::Field(f) => matches!(&f.member,syn::Member::Named(n) if n=="opcode"),
        Expr::Path(p) => p.path.is_ident("opcode"),
        Expr::MethodCall(m) => m.method == "opcode" && m.args.is_empty(),
        Expr::Paren(p) => opcode(&p.expr),
        Expr::Reference(r) => opcode(&r.expr),
        _ => false,
    }
}

// matches! has a pattern and an optional guard rather than a list of expressions.
struct Matches {
    subject: Expr,
    pattern: Pat,
    guard: Option<Expr>,
}
impl Parse for Matches {
    fn parse(tokens: ParseStream<'_>) -> syn::Result<Self> {
        let subject = tokens.parse()?;
        tokens.parse::<Token![,]>()?;
        let pattern = Pat::parse_multi_with_leading_vert(tokens)?;
        let guard = if tokens.peek(Token![if]) {
            tokens.parse::<Token![if]>()?;
            Some(tokens.parse()?)
        } else {
            None
        };
        let _ = tokens.parse::<Option<Token![,]>>()?;
        Ok(Self {
            subject,
            pattern,
            guard,
        })
    }
}

#[derive(Default)]
struct Audit {
    message_impl: bool,
    function: String,
    failures: Vec<String>,
}

impl Audit {
    fn report(&mut self, n: Option<u16>, use_site: &str) {
        if let Some(raw) = n
            && let Ok(op) = Opcode::try_from(raw)
        {
            self.failures.push(format!(
                "{}: {use_site} uses {raw:#06x} ({op:?})",
                self.function
            ));
        }
    }
    fn pair(&mut self, left: &Expr, right: &Expr) {
        if opcode(left) {
            self.report(integer(right), "opcode comparison");
        }
        if opcode(right) {
            self.report(integer(left), "opcode comparison");
        }
    }
    fn pattern(&mut self, subject: &Expr, pattern: &Pat) {
        match pattern {
            Pat::Or(or) => {
                for case in &or.cases {
                    self.pattern(subject, case)
                }
            }
            Pat::Paren(p) => self.pattern(subject, &p.pat),
            Pat::Ident(p) => {
                if let Some((_, binding)) = &p.subpat {
                    self.pattern(subject, binding);
                }
            }
            Pat::Tuple(p) => {
                if let Expr::Tuple(expr) = subject {
                    for (expr, pat) in expr.elems.iter().zip(&p.elems) {
                        self.pattern(expr, pat);
                    }
                }
            }
            Pat::Lit(lit) if opcode(subject) => {
                if let Lit::Int(n) = &lit.lit {
                    self.report(n.base10_parse().ok(), "opcode pattern");
                }
            }
            Pat::Range(range) if opcode(subject) => {
                for end in [range.start.as_deref(), range.end.as_deref()]
                    .into_iter()
                    .flatten()
                {
                    self.report(integer(end), "opcode range");
                }
            }
            _ => {}
        }
    }
}

impl<'ast> Visit<'ast> for Audit {
    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        let prior = self.message_impl;
        self.message_impl = matches!(&*i.self_ty,syn::Type::Path(t) if t.path.segments.last().is_some_and(|s|s.ident=="Message"));
        visit::visit_item_impl(self, i);
        self.message_impl = prior;
    }
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        let old = std::mem::replace(&mut self.function, f.sig.ident.to_string());
        visit::visit_item_fn(self, f);
        self.function = old;
    }
    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        let old = std::mem::replace(&mut self.function, f.sig.ident.to_string());
        visit::visit_impl_item_fn(self, f);
        self.function = old;
    }
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Expr::Path(p) = &*call.func {
            let mut names = p.path.segments.iter().rev();
            if names.next().is_some_and(|s| s.ident == "new")
                && names
                    .next()
                    .is_some_and(|s| s.ident == "Message" || self.message_impl && s.ident == "Self")
            {
                self.report(call.args.first().and_then(integer), "message construction");
            }
        }
        visit::visit_expr_call(self, call);
    }
    fn visit_expr_binary(&mut self, b: &'ast syn::ExprBinary) {
        if matches!(
            b.op,
            BinOp::Eq(_) | BinOp::Ne(_) | BinOp::Lt(_) | BinOp::Le(_) | BinOp::Gt(_) | BinOp::Ge(_)
        ) {
            self.pair(&b.left, &b.right);
        }
        visit::visit_expr_binary(self, b);
    }
    fn visit_expr_match(&mut self, m: &'ast syn::ExprMatch) {
        for arm in &m.arms {
            self.pattern(&m.expr, &arm.pat);
        }
        visit::visit_expr_match(self, m);
    }
    fn visit_expr_let(&mut self, l: &'ast syn::ExprLet) {
        self.pattern(&l.expr, &l.pat);
        visit::visit_expr_let(self, l);
    }
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        let Some(last) = m.path.segments.last() else {
            return;
        };
        match last.ident.to_string().as_str() {
            "matches" => {
                if let Ok(input) = syn::parse2::<Matches>(m.tokens.clone()) {
                    self.pattern(&input.subject, &input.pattern);
                    self.visit_expr(&input.subject);
                    if let Some(guard) = &input.guard {
                        self.visit_expr(guard);
                    }
                }
            }
            name @ ("assert" | "assert_eq" | "assert_ne" | "debug_assert" | "debug_assert_eq"
            | "debug_assert_ne" | "ensure" | "vec") => {
                if let Ok(args) =
                    Punctuated::<Expr, Token![,]>::parse_terminated.parse2(m.tokens.clone())
                {
                    if (name.ends_with("_eq") || name.ends_with("_ne")) && args.len() >= 2 {
                        self.pair(&args[0], &args[1]);
                    }
                    for arg in args {
                        self.visit_expr(&arg);
                    }
                }
            }
            _ => {}
        }
    }
}

fn inspect(path: &Path, failures: &mut Vec<String>) {
    for entry in fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if entry.file_type().unwrap().is_dir() {
            if entry.file_name() != "target" {
                inspect(&path, failures);
            }
        } else if path.extension().is_some_and(|s| s == "rs") {
            let file = syn::parse_file(&fs::read_to_string(&path).unwrap())
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let mut audit = Audit::default();
            audit.visit_file(&file);
            failures.extend(
                audit
                    .failures
                    .into_iter()
                    .map(|f| format!("{}: {f}", path.display())),
            );
        }
    }
}

#[test]
fn workspace_message_boundaries_use_named_opcodes() {
    let members = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let mut failures = Vec::new();
    inspect(&members, &mut failures);
    failures.sort();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn recognizes_known_literals_only_in_their_opcode_position() {
    let source = r#"
        impl Message { fn from_fixture(){ Self::new(0x3ee,[]); } }
        fn example(m:Message,opcode:u16) {
            halcyon_protocol::Message::new(0, []);
            assert!(m.opcode != 0x041a);
            assert_eq!(5,opcode);
            match (m.opcode,m.body.len()) { (0x041b | 0b110,0)=>(), _=>() }
            if matches!(m.opcode,0x3e9..=0x3ec if m.body.len()==0) {}
            if let 0x043A = m.opcode {}
            if matches!(m.body.len(),0 if m.opcode==0x3f2) {}
        }"#;
    let mut audit = Audit::default();
    audit.visit_file(&syn::parse_file(source).unwrap());
    assert_eq!(audit.failures.len(), 10, "{:?}", audit.failures);
}

#[test]
fn ignores_nonopcode_data_and_unknown_wire_codes() {
    let source = r#"fn fixture(m:Message){            let _="Message::new(0x0415, [])";
            // m.opcode==0
            let golden=[0,0x04,0x15,0];
            assert_eq!(golden[0],0);
            Message::new(0xffee,[0,5,6]);
            Message::known(Opcode::PlayerInfo,[0,5,6]);
            match (m.opcode,m.body.len()){(_,0)=>(),_=>()}
 }"#;
    let mut audit = Audit::default();
    audit.visit_file(&syn::parse_file(source).unwrap());
    assert!(audit.failures.is_empty(), "{:?}", audit.failures);
}

use serde::Serialize;

#[derive(Serialize)]
pub struct RpcEnvelope<'a, P> {
    pub method: &'a str,
    pub params: P,
}

/// Positional arguments sent by the archived Android guest-creation call.
pub fn android_guest<'a>(device: &'a str, instance: &'a str) -> impl Serialize + 'a {
    (
        "kindred",
        "en",
        "US",
        "production",
        "en",
        "BAKLAVA",
        "android",
        "",
        "147219",
        1,
        device,
        "model",
        instance,
    )
}

use std::time::Duration;

/// Riot's local-API root certificate, vendored from the Irelia crate (MIT), which ships Riot's
/// published `riotgames.pem` unchanged.
const RIOT_ROOT_PEM: &[u8] = include_bytes!("riotgames.pem");

/// An HTTPS client that trusts *only* the Riot root, rather than disabling verification.
/// Proxies are ignored: these are loopback APIs, and a system proxy must never see them.
pub(crate) fn riot_client(timeout: Duration) -> reqwest::Client {
    let cert = reqwest::Certificate::from_pem(RIOT_ROOT_PEM).expect("vendored Riot certificate is valid PEM");
    reqwest::Client::builder()
        .tls_built_in_root_certs(false)
        .add_root_certificate(cert)
        .no_proxy()
        .timeout(timeout)
        .connect_timeout(Duration::from_millis(500))
        .build()
        .expect("TLS client configuration is static and valid")
}

#[cfg(test)]
mod tests {
    #[test]
    fn client_builds_with_the_vendored_certificate() {
        let _ = super::riot_client(std::time::Duration::from_secs(1));
    }
}

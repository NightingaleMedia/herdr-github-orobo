use std::time::Duration;

pub fn agent_for(url: &str) -> Result<ureq::Agent, String> {
    let mut builder = ureq::AgentBuilder::new().timeout(Duration::from_secs(20));

    #[cfg(target_os = "macos")]
    {
        let decision = proxyparser::find_proxy_for_url(url)
            .map_err(|e| format!("resolve proxy for {url}: {e}"))?;
        if let Some(proxy_url) = decision
            .split(';')
            .map(str::trim)
            .find(|part| !part.is_empty() && !part.eq_ignore_ascii_case("DIRECT"))
        {
            let proxy = ureq::Proxy::new(proxy_url)
                .map_err(|e| format!("invalid proxy {proxy_url}: {e}"))?;
            builder = builder.proxy(proxy);
        }
    }

    Ok(builder.build())
}

#[derive(Default)]
pub struct Analytics {
    pub enabled: bool,
    pub project: String,
    pub key: String,
    pub endpoint: String,
    pub origin: String,
}

impl Analytics {
    pub fn from_env() -> Self {
        Self::configured(
            std::env::var("HOUSE_EDGE_ENABLED")
                .unwrap_or_default()
                .trim()
                == "true",
            std::env::var("HOUSE_EDGE_PROJECT").unwrap_or_else(|_| "plant-journal".into()),
            std::env::var("HOUSE_EDGE_KEY").unwrap_or_default(),
            std::env::var("HOUSE_EDGE_ENDPOINT").unwrap_or_default(),
        )
    }

    fn configured(enabled: bool, project: String, key: String, endpoint: String) -> Self {
        if !enabled || key.trim().is_empty() || project.trim().is_empty() {
            return Self::default();
        }
        let Ok(url) = reqwest::Url::parse(endpoint.trim()) else {
            return Self::default();
        };
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Self::default();
        }
        Self {
            enabled: true,
            project,
            key: key.trim().into(),
            endpoint: url.to_string(),
            origin: url.origin().ascii_serialization(),
        }
    }

    pub fn content_security_policy(&self) -> String {
        let collector = if self.enabled {
            format!(" {}", self.origin)
        } else {
            String::new()
        };
        format!("default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'{collector}; frame-ancestors 'none'; base-uri 'self'; form-action 'self'")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analytics_requires_explicit_valid_browser_configuration() {
        for (enabled, key, endpoint) in [
            (false, "key", "https://example.com/api/collect"),
            (true, "", "https://example.com/api/collect"),
            (true, "key", "invalid"),
            (true, "key", "javascript:alert(1)"),
            (true, "key", "https://user:password@example.com/api/collect"),
        ] {
            let config =
                Analytics::configured(enabled, "plant-journal".into(), key.into(), endpoint.into());
            assert!(!config.enabled);
            assert!(config
                .content_security_policy()
                .contains("connect-src 'self';"));
        }
    }

    #[test]
    fn csp_allows_only_the_collector_origin() {
        let config = Analytics::configured(
            true,
            "plant-journal".into(),
            "key".into(),
            "https://analytics.example.com:8443/api/collect".into(),
        );
        assert!(config.enabled);
        assert_eq!(config.origin, "https://analytics.example.com:8443");
        assert!(config
            .content_security_policy()
            .contains("connect-src 'self' https://analytics.example.com:8443;"));
        assert!(config
            .content_security_policy()
            .contains("script-src 'self';"));
    }
}

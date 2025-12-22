use anyhow::Result;

#[derive(Clone, Debug)]
pub struct JailInfo {
    pub jid: u32,
    pub name: String,
    pub hostname: String,
    pub ip_addresses: Vec<String>,
    pub path: String,
}

pub struct JailCollector;

impl JailCollector {
    pub fn new() -> Self {
        Self
    }

    pub fn collect(&self) -> Result<Vec<JailInfo>> {
        // Use jls to list running jails
        let output = std::process::Command::new("jls")
            .arg("-n")
            .arg("-h")
            .arg("jid")
            .arg("name")
            .arg("host.hostname")
            .arg("ip4.addr")
            .arg("path")
            .output()?;

        let output_str = String::from_utf8_lossy(&output.stdout);
        let mut jails = Vec::new();

        for line in output_str.lines().skip(1) {
            // Skip header line
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if let Some(jail_info) = self.parse_jls_line(line) {
                jails.push(jail_info);
            }
        }

        Ok(jails)
    }

    fn parse_jls_line(&self, line: &str) -> Option<JailInfo> {
        // Format: jid name host.hostname ip4.addr path
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            return None;
        }

        let jid: u32 = parts[0].parse().ok()?;
        let name = parts[1].to_string();
        let hostname = parts[2].to_string();

        // IP addresses might be comma-separated or just one
        let ip_str = parts.get(3).unwrap_or(&"-");
        let ip_addresses: Vec<String> = if ip_str != &"-" {
            ip_str.split(',').map(|s| s.to_string()).collect()
        } else {
            vec![]
        };

        let path = parts.get(4).unwrap_or(&"-").to_string();

        Some(JailInfo {
            jid,
            name,
            hostname,
            ip_addresses,
            path,
        })
    }
}

impl Default for JailCollector {
    fn default() -> Self {
        Self::new()
    }
}

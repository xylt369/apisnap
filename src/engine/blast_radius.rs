use crate::config::EndpointConfig;
use crate::engine::DiffKind;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Severity of a cascading blast radius impact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlastSeverity {
    Critical, // Removed field or TypeMismatch on consumed path (will crash downstream)
    Warning,  // Value change or Added field on consumed path
}

/// A specific downstream consumer endpoint impacted by an upstream change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BlastRadiusFinding {
    pub affected_endpoint: String,
    pub affected_team: Option<String>,
    pub triggering_paths: Vec<String>,
    pub severity: BlastSeverity,
}

/// Complete Blast Radius Analysis Report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlastRadiusReport {
    pub changed_endpoint: String,
    pub modified_paths: Vec<String>,
    pub findings: Vec<BlastRadiusFinding>,
}

impl BlastRadiusReport {
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }

    pub fn format_markdown(&self) -> String {
        if self.is_clean() {
            return format!(
                "### 🛡️ Blast Radius: 0 Downstream Breaches\n\nNo registered downstream services are impacted by changes to `{}`.",
                self.changed_endpoint
            );
        }

        let mut out = format!(
            "### ⚠️ Blast Radius Alert: Downstream Impact on `{}`\n\n",
            self.changed_endpoint
        );
        out.push_str("| Affected Consumer Service | Owning Team | Severity | Triggering JSONPaths |\n");
        out.push_str("| :--- | :--- | :--- | :--- |\n");

        for f in &self.findings {
            let team = f.affected_team.as_deref().unwrap_or("Unassigned");
            let sev = match f.severity {
                BlastSeverity::Critical => "🔴 **CRITICAL (Breaking)**",
                BlastSeverity::Warning => "🟡 **WARNING**",
            };
            let paths = f.triggering_paths.join(", ");
            out.push_str(&format!(
                "| `{}` | {} | {} | `{}` |\n",
                f.affected_endpoint, team, sev, paths
            ));
        }

        out
    }
}

/// Blast Radius Calculator engine.
pub struct BlastRadiusCalculator;

impl BlastRadiusCalculator {
    pub fn calculate(
        changed_endpoint: &str,
        diffs: &[DiffKind],
        all_endpoints: &[EndpointConfig],
    ) -> BlastRadiusReport {
        let mut modified_paths = Vec::new();
        let mut critical_paths = HashSet::new();
        let mut all_diff_paths = HashSet::new();

        for diff in diffs {
            let (path, is_critical) = match diff {
                DiffKind::Removed { json_path, .. } => (json_path.clone(), true),
                DiffKind::TypeMismatch { json_path, .. } => (json_path.clone(), true),
                DiffKind::Added { json_path, .. } => (json_path.clone(), false),
                DiffKind::Modified { json_path, .. } => (json_path.clone(), false),
                DiffKind::ArrayLengthMismatch { json_path, .. } => (json_path.clone(), true),
                DiffKind::DepthExceeded { json_path, .. } => (json_path.clone(), true),
            };

            modified_paths.push(path.clone());
            if is_critical {
                critical_paths.insert(path.clone());
            }
            all_diff_paths.insert(path);
        }

        let mut findings = Vec::new();

        for consumer in all_endpoints {
            for dep in &consumer.upstream_dependencies {
                if dep.upstream_endpoint == changed_endpoint {
                    let mut triggered_paths = Vec::new();
                    let mut has_critical = false;

                    if dep.consumed_json_paths.is_empty() {
                        // Consumes entire endpoint!
                        triggered_paths = modified_paths.clone();
                        has_critical = !critical_paths.is_empty();
                    } else {
                        for consumed_path in &dep.consumed_json_paths {
                            if all_diff_paths.contains(consumed_path) {
                                triggered_paths.push(consumed_path.clone());
                                if critical_paths.contains(consumed_path) {
                                    has_critical = true;
                                }
                            }
                        }
                    }

                    if !triggered_paths.is_empty() {
                        findings.push(BlastRadiusFinding {
                            affected_endpoint: consumer.name.clone(),
                            affected_team: dep.owning_team.clone(),
                            triggering_paths: triggered_paths,
                            severity: if has_critical {
                                BlastSeverity::Critical
                            } else {
                                BlastSeverity::Warning
                            },
                        });
                    }
                }
            }
        }

        BlastRadiusReport {
            changed_endpoint: changed_endpoint.to_string(),
            modified_paths,
            findings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{HttpMethod, Protocol, UpstreamDependency};
    use std::collections::HashMap;

    #[test]
    fn test_blast_radius_critical_cascade() {
        let changed_ep = "user_service.get_profile";

        let diffs = vec![
            DiffKind::Removed {
                json_path: "$.user.email".into(),
                old_value: serde_json::json!("alice@example.com"),
            },
            DiffKind::Added {
                json_path: "$.user.avatar_url".into(),
                new_value: serde_json::json!("https://img.io/1.png"),
            },
        ];

        let order_service = EndpointConfig {
            name: "order_service.checkout".into(),
            protocol: Protocol::Http,
            method: HttpMethod::Post,
            path: "/checkout".into(),
            grpc: None,
            headers: HashMap::new(),
            query_params: HashMap::new(),
            body: None,
            expected_status: 200,
            timeout_override: None,
            float_epsilon_override: None,
            auth_override: None,
            mask_overrides: Vec::new(),
            array_modes: HashMap::new(),
            upstream_dependencies: vec![UpstreamDependency {
                upstream_endpoint: "user_service.get_profile".into(),
                consumed_json_paths: vec!["$.user.email".into()],
                owning_team: Some("orders-team".into()),
            }],
        };

        let report = BlastRadiusCalculator::calculate(changed_ep, &diffs, &[order_service]);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].affected_endpoint, "order_service.checkout");
        assert_eq!(report.findings[0].severity, BlastSeverity::Critical);
        assert_eq!(report.findings[0].affected_team.as_deref(), Some("orders-team"));
    }
}

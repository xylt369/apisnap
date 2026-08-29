use crate::engine::DiffReport;
use colored::Colorize;
use serde::Serialize;

#[derive(Serialize)]
struct CiSummaryReport<'a> {
    total_endpoints: usize,
    passed_count: usize,
    failed_count: usize,
    duration_ms: u64,
    all_passed: bool,
    reports: &'a [DiffReport],
}

/// Print formatted test execution results to stdout.
pub fn print_summary_report(reports: &[DiffReport], total_duration_ms: u64, is_ci: bool) {
    let passed_count = reports.iter().filter(|r| r.passed()).count();
    let failed_count = reports.len() - passed_count;
    let all_passed = failed_count == 0;

    if is_ci {
        let ci_report = CiSummaryReport {
            total_endpoints: reports.len(),
            passed_count,
            failed_count,
            duration_ms: total_duration_ms,
            all_passed,
            reports,
        };
        let json_str = serde_json::to_string_pretty(&ci_report).unwrap_or_default();
        println!("{json_str}");
        return;
    }

    println!("\n{}", "=".repeat(60).cyan());
    println!(
        "{}  {}",
        "ApiSnap Test Execution Summary".bold(),
        format!("({total_duration_ms}ms)").dimmed()
    );
    println!("{}\n", "=".repeat(60).cyan());

    for report in reports {
        if report.passed() {
            println!(
                "  {} {:<35} (HTTP {})",
                "[PASS]".green().bold(),
                report.endpoint_name.bold(),
                report.actual_status.to_string().dimmed()
            );
        } else {
            println!(
                "  {} {:<35} ({} diffs, status: {} -> {})",
                "[FAIL]".red().bold(),
                report.endpoint_name.bold(),
                report.differences.len(),
                report.expected_status,
                report.actual_status
            );
            println!("{}", report.render_colored());
        }
    }

    println!("\n{}", "-".repeat(60).dimmed());
    println!(
        "Results: {} total | {} passed | {} failed",
        reports.len().to_string().bold(),
        passed_count.to_string().green().bold(),
        if failed_count > 0 {
            failed_count.to_string().red().bold()
        } else {
            "0".to_string().green().bold()
        }
    );
    println!("{}\n", "-".repeat(60).dimmed());
}

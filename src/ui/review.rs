use crate::engine::DiffReport;
use crate::error::ApiSnapError;
use crate::snapshot::{SnapshotFile, SnapshotStore};
use colored::Colorize;
use std::io::{self, BufRead, Write};

pub struct ReviewItem {
    pub report: DiffReport,
    pub new_snapshot: SnapshotFile,
}

pub enum ReviewDecision {
    Accept,
    Reject,
    Skip,
    Quit,
}

/// Run an interactive CLI review session over all detected diffs.
pub fn run_interactive_review(
    items: &[ReviewItem],
    store: &SnapshotStore,
) -> Result<bool, ApiSnapError> {
    if items.is_empty() {
        println!("\n{}", "No snapshot diffs to review. All endpoints match!".green().bold());
        return Ok(true);
    }

    println!("\n{}", "=".repeat(60).cyan());
    println!(
        "{} ({} mismatch(es) detected)",
        "ApiSnap Interactive Snapshot Review".bold(),
        items.len()
    );
    println!("{}\n", "=".repeat(60).cyan());

    let mut accepted_count = 0;
    let mut rejected_count = 0;
    let mut skipped_count = 0;

    let stdin = io::stdin();
    let mut handle = stdin.lock();

    for (idx, item) in items.iter().enumerate() {
        println!(
            "\n[{}/{}] Reviewing: {}",
            idx + 1,
            items.len(),
            item.report.endpoint_name.cyan().bold()
        );
        println!("{}", item.report.render_colored());

        loop {
            print!(
                "  Decision [{}ccept / {}eject / {}kip / {}uit]: ",
                "(a)".green().bold(),
                "(r)".red().bold(),
                "(s)".yellow().bold(),
                "(q)".dimmed().bold()
            );
            io::stdout().flush().map_err(|e| ApiSnapError::Io {
                path: "stdout".into(),
                source: e,
            })?;

            let mut input = String::new();
            handle.read_line(&mut input).map_err(|e| ApiSnapError::Io {
                path: "stdin".into(),
                source: e,
            })?;

            let trimmed = input.trim().to_lowercase();
            match trimmed.as_str() {
                "a" | "accept" => {
                    let written_path = store.write_snapshot_atomic(&item.new_snapshot)?;
                    println!(
                        "  {} Snapshot updated: {}",
                        "[ACCEPTED]".green().bold(),
                        written_path.display().to_string().dimmed()
                    );
                    accepted_count += 1;
                    break;
                }
                "r" | "reject" => {
                    println!("  {} Snapshot left unchanged.", "[REJECTED]".red().bold());
                    rejected_count += 1;
                    break;
                }
                "s" | "skip" => {
                    println!("  {} Review deferred.", "[SKIPPED]".yellow().bold());
                    skipped_count += 1;
                    break;
                }
                "q" | "quit" => {
                    println!("\n{}", "Review aborted by user.".yellow().bold());
                    return Ok(false);
                }
                _ => {
                    println!("  Invalid choice. Please enter 'a', 'r', 's', or 'q'.");
                }
            }
        }
    }

    println!("\n{}", "-".repeat(60).dimmed());
    println!(
        "Review Complete: {} accepted, {} rejected, {} skipped",
        accepted_count.to_string().green().bold(),
        rejected_count.to_string().red().bold(),
        skipped_count.to_string().yellow().bold()
    );
    println!("{}\n", "-".repeat(60).dimmed());

    // All clean if no rejections or skips remain
    Ok(rejected_count == 0 && skipped_count == 0)
}

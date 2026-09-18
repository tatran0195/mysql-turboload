//! UI layout, styling, and card formatting module for Zephyr.

use colored::Colorize;
use regex::Regex;
use std::sync::OnceLock;

static ANSI_REGEX: OnceLock<Regex> = OnceLock::new();

/// Returns a string with ANSI escape codes stripped out.
pub fn strip_ansi(s: &str) -> String {
    let re = ANSI_REGEX.get_or_init(|| Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap());
    re.replace_all(s, "").to_string()
}

/// Calculates the visible width of a string in terminal columns, ignoring ANSI escape sequences.
pub fn visible_width(s: &str) -> usize {
    strip_ansi(s).chars().count()
}

/// Helper to format a float with thousands separators (commas) before the decimal point.
pub fn format_with_commas(val: f64) -> String {
    let s = format!("{val:.2}");
    let parts: Vec<&str> = s.split('.').collect();
    let int_part = parts[0];
    let dec_part = parts.get(1).copied().unwrap_or("00");

    let is_neg = int_part.starts_with('-');
    let raw_digits = if is_neg { &int_part[1..] } else { int_part };

    let mut result = String::new();
    let len = raw_digits.len();
    for (i, ch) in raw_digits.chars().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }

    if is_neg {
        format!("-{result}.{dec_part}")
    } else {
        format!("{result}.{dec_part}")
    }
}

/// Formats a byte count into a human-readable string.
/// For sizes >= 1 GB, includes both GB and formatted MB in parentheses (e.g. `1.81 GB (1,851.58 MB)`).
pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.2} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        let gb = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        let mb = bytes as f64 / (1024.0 * 1024.0);
        format!("{gb:.2} GB ({})", format_with_commas(mb) + " MB")
    }
}

/// Compact byte formatting (e.g. `45.2 MB`, `1.81 GB`).
pub fn format_bytes_compact(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Shortens a path or long string with an ellipsis in the middle if it exceeds max_chars.
pub fn truncate_path_str(path: &str, max_chars: usize) -> String {
    let count = path.chars().count();
    if count <= max_chars {
        return path.to_string();
    }
    let keep = max_chars.saturating_sub(3);
    let half = keep / 2;
    let start: String = path.chars().take(half).collect();
    let end: String = path.chars().skip(count - (keep - half)).collect();
    format!("{start}...{end}")
}

/// Renders a framed startup card with cyan borders, dynamically sizing to content.
pub fn render_card(
    title: &str,
    version: &str,
    subtitle: &str,
    items: &[(&str, String)],
) -> String {
    let mut out = String::new();

    // Determine required card inner width (min 61)
    let max_item_len = items
        .iter()
        .map(|(label, value)| visible_width(&format!("  • {:<15} : {value}", label)))
        .max()
        .unwrap_or(0);
    let title_vis_len = 2 + visible_width(title) + 1 + visible_width(&format!("v{version}"));
    let sub_vis_len = 2 + visible_width(subtitle);
    let inner_width = max_item_len.max(title_vis_len).max(sub_vis_len).max(61) + 2;

    // Top border
    let top_border = format!("╭{}╮", "─".repeat(inner_width)).cyan().to_string();
    out.push_str(&top_border);
    out.push('\n');

    // Title line: │  TITLE version ... │
    let title_styled = title.cyan().bold().to_string();
    let version_styled = format!("v{version}").green().bold().to_string();
    let header_content = format!("  {title_styled} {version_styled}");
    let header_vis = visible_width(&header_content);
    let pad_len = inner_width.saturating_sub(header_vis);
    out.push_str(&format!("│{header_content}{}│\n", " ".repeat(pad_len)));

    // Subtitle line
    let sub_content = format!("  {}", subtitle.cyan());
    let sub_vis = visible_width(&sub_content);
    let pad_len = inner_width.saturating_sub(sub_vis);
    out.push_str(&format!("│{sub_content}{}│\n", " ".repeat(pad_len)));

    // Mid divider
    let mid_divider = format!("├{}┤", "─".repeat(inner_width)).cyan().to_string();
    out.push_str(&mid_divider);
    out.push('\n');

    // Items
    for (label, value) in items {
        let bullet = "•".dimmed();
        let item_content = format!("  {bullet} {:<15} : {value}", label);
        let item_vis = visible_width(&item_content);
        let pad_len = inner_width.saturating_sub(item_vis);
        out.push_str(&format!("│{item_content}{}│\n", " ".repeat(pad_len)));
    }

    // Bottom border
    let bottom_border = format!("╰{}╯", "─".repeat(inner_width)).cyan().to_string();
    out.push_str(&bottom_border);

    out
}

/// Renders a completion summary card with color corresponding to outcome, dynamically sizing to content.
pub fn render_summary_card(
    status_title: &str,
    is_success: bool,
    is_cancelled: bool,
    metrics: &[(&str, String)],
) -> String {
    let mut out = String::new();

    // Determine required card inner width (min 61)
    let max_metric_len = metrics
        .iter()
        .map(|(label, value)| visible_width(&format!("  • {:<16} : {value}", label)))
        .max()
        .unwrap_or(0);
    let status_vis = 2 + visible_width(status_title);
    let inner_width = max_metric_len.max(status_vis).max(61) + 2;

    let (top_char, mid_char, bot_char) = ("╭", "├", "╰");
    let (top_right, mid_right, bot_right) = ("╮", "┤", "╯");

    let top_border = if is_cancelled {
        format!("{}{}{}", top_char, "─".repeat(inner_width), top_right).yellow().to_string()
    } else if is_success {
        format!("{}{}{}", top_char, "─".repeat(inner_width), top_right).green().to_string()
    } else {
        format!("{}{}{}", top_char, "─".repeat(inner_width), top_right).red().to_string()
    };
    out.push_str(&top_border);
    out.push('\n');

    // Status Header
    let status_content = if is_cancelled {
        format!("  {status_title}").yellow().bold().to_string()
    } else if is_success {
        format!("  {status_title}").green().bold().to_string()
    } else {
        format!("  {status_title}").red().bold().to_string()
    };
    let status_vis_len = visible_width(&status_content);
    let pad_len = inner_width.saturating_sub(status_vis_len);
    out.push_str(&format!("│{status_content}{}│\n", " ".repeat(pad_len)));

    // Mid Divider
    let mid_divider = if is_cancelled {
        format!("{}{}{}", mid_char, "─".repeat(inner_width), mid_right).yellow().to_string()
    } else if is_success {
        format!("{}{}{}", mid_char, "─".repeat(inner_width), mid_right).green().to_string()
    } else {
        format!("{}{}{}", mid_char, "─".repeat(inner_width), mid_right).red().to_string()
    };
    out.push_str(&mid_divider);
    out.push('\n');

    // Metrics
    for (label, value) in metrics {
        let bullet = "•".dimmed();
        let metric_content = format!("  {bullet} {:<16} : {value}", label);
        let metric_vis = visible_width(&metric_content);
        let pad_len = inner_width.saturating_sub(metric_vis);
        out.push_str(&format!("│{metric_content}{}│\n", " ".repeat(pad_len)));
    }

    // Bottom border
    let bottom_border = if is_cancelled {
        format!("{}{}{}", bot_char, "─".repeat(inner_width), bot_right).yellow().to_string()
    } else if is_success {
        format!("{}{}{}", bot_char, "─".repeat(inner_width), bot_right).green().to_string()
    } else {
        format!("{}{}{}", bot_char, "─".repeat(inner_width), bot_right).red().to_string()
    };
    out.push_str(&bottom_border);

    out
}

/// Renders a modern section divider (e.g. `─── Starting Parallel Import (6 Workers) ───────────────────────────`).
pub fn render_section_divider(title: &str) -> String {
    let total_width: usize = 65;
    let prefix = "─── ";
    let suffix_spacing = " ";
    let title_vis_len = visible_width(title);
    let used_width = prefix.chars().count() + title_vis_len + suffix_spacing.chars().count();
    let remaining_width = total_width.saturating_sub(used_width).max(3);

    format!("{prefix}{title}{suffix_spacing}{}", "─".repeat(remaining_width))
}

/// Renders a database breakdown row with aligned dots, count, and MB size.
pub fn render_database_row(
    db_name: &str,
    count: usize,
    total_bytes: u64,
    skipped_count: Option<usize>,
) -> String {
    let arrow = "▸".cyan();
    let mb = (total_bytes as f64) / (1024.0 * 1024.0);
    let mb_str = format!("{mb:>10.2} MB");

    let count_str = if let Some(skipped) = skipped_count {
        if skipped > 0 {
            format!("{:>4} files ({} skipped)", count, skipped.to_string().yellow())
        } else {
            format!("{:>4} files", count)
        }
    } else {
        format!("{:>4} files", count)
    };

    // Fixed database name column width: 22 chars
    let db_col_width = 22;
    let db_vis_len = visible_width(db_name);
    let leader_dots = if db_vis_len < db_col_width {
        "·".repeat(db_col_width - db_vis_len)
    } else {
        "··".to_string()
    };

    format!(
        "  {} {} {}  {:<26}  {}",
        arrow,
        db_name.cyan(),
        leader_dots.dimmed(),
        count_str,
        mb_str
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes_representation() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024 * 1024), "1.00 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.00 GB (1,024.00 MB)");
    }

    #[test]
    fn test_format_with_commas() {
        assert_eq!(format_with_commas(1851.58), "1,851.58");
        assert_eq!(format_with_commas(100.5), "100.50");
        assert_eq!(format_with_commas(1234567.89), "1,234,567.89");
    }

    #[test]
    fn test_render_card_structure() {
        let items = [("Target Server", "127.0.0.1:3306".to_string())];
        let card = render_card("ZEPHYR", "1.0.0", "Concurrent Engine", &items);
        assert!(card.contains("╭"));
        assert!(card.contains("ZEPHYR"));
        assert!(card.contains("v1.0.0"));
        assert!(card.contains("Target Server"));
        assert!(card.contains("╰"));
    }

    #[test]
    fn test_render_summary_card_colors() {
        let metrics = [("Tables Imported", "106 / 106".to_string())];
        let success_card = render_summary_card("✔ IMPORT COMPLETED", true, false, &metrics);
        assert!(success_card.contains("✔ IMPORT COMPLETED"));

        let fail_card = render_summary_card("✖ IMPORT FAILED", false, false, &metrics);
        assert!(fail_card.contains("✖ IMPORT FAILED"));
    }

    #[test]
    fn test_render_section_divider() {
        let div = render_section_divider("Starting Parallel Import (6 Workers)");
        assert!(div.starts_with("─── Starting Parallel Import (6 Workers) ─"));
        assert_eq!(div.chars().count(), 65);
    }

    #[test]
    fn test_render_database_row() {
        let row = render_database_row("9837987", 106, 1024 * 1024 * 1851, None);
        assert!(row.contains("9837987"));
        assert!(row.contains("106 files"));
        assert!(row.contains("1851.00 MB"));
    }
}

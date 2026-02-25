//! ASCII art banner for cosq CLI

use colored::Colorize;

const LOGO: &str = r#"
  ██████╗ ██████╗ ███████╗ ██████╗
 ██╔════╝██╔═══██╗██╔════╝██╔═══██╗
 ██║     ██║   ██║███████╗██║   ██║
 ██║     ██║   ██║╚════██║██║▄▄ ██║
 ╚██████╗╚██████╔╝███████║╚██████╔╝
  ╚═════╝ ╚═════╝ ╚══════╝ ╚══▀▀═╝"#;

/// Print the cosq ASCII art banner
pub fn print_banner() {
    for line in LOGO.lines() {
        println!("{}", line.bold());
    }
}

/// Print the banner with version info
pub fn print_banner_with_version() {
    print_banner();
    println!(
        " {} {}",
        "Query your Azure Cosmos DB instances".dimmed(),
        format!("v{}", env!("CARGO_PKG_VERSION")).dimmed(),
    );
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logo_is_not_empty() {
        assert!(!LOGO.is_empty());
    }

    #[test]
    fn test_logo_has_six_lines() {
        let lines: Vec<&str> = LOGO.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 6, "Logo should have 6 lines of block letters");
    }
}

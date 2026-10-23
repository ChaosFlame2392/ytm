use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Browser {
    Safari,
    Firefox,
    Chrome,
    Brave,
    Edge,
    Chromium,
    Opera,
    Vivaldi,
    Whale,
}

impl Browser {
    pub fn as_str(&self) -> &'static str {
        match self {
            Browser::Safari => "safari",
            Browser::Firefox => "firefox",
            Browser::Chrome => "chrome",
            Browser::Brave => "brave",
            Browser::Edge => "edge",
            Browser::Chromium => "chromium",
            Browser::Opera => "opera",
            Browser::Vivaldi => "vivaldi",
            Browser::Whale => "whale",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "safari" => Some(Browser::Safari),
            "firefox" => Some(Browser::Firefox),
            "chrome" => Some(Browser::Chrome),
            "brave" => Some(Browser::Brave),
            "edge" => Some(Browser::Edge),
            "chromium" => Some(Browser::Chromium),
            "opera" => Some(Browser::Opera),
            "vivaldi" => Some(Browser::Vivaldi),
            "whale" => Some(Browser::Whale),
            _ => None,
        }
    }
}

impl fmt::Display for Browser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for Browser {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or_else(|| format!("Unsupported browser identifier: {}", s))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CookieSource {
    Browser(Browser),
    File(std::path::PathBuf),
}

impl CookieSource {
    /// Returns the arguments to pass to yt-dlp CLI
    pub fn ytdlp_args(&self) -> Vec<String> {
        match self {
            CookieSource::Browser(browser) => {
                vec!["--cookies-from-browser".to_string(), browser.as_str().to_string()]
            }
            CookieSource::File(path) => {
                vec!["--cookies".to_string(), path.to_string_lossy().into_owned()]
            }
        }
    }

    /// Returns the mpv raw ytdl option argument
    pub fn mpv_arg(&self) -> String {
        match self {
            CookieSource::Browser(browser) => {
                format!("--ytdl-raw-options=cookies-from-browser={}", browser.as_str())
            }
            CookieSource::File(path) => {
                format!("--ytdl-raw-options=cookies={}", path.to_string_lossy())
            }
        }
    }

    /// Resolves cookie source from explicit inputs, enforcing the resolution hierarchy:
    /// 1. `cookies_file_var`: if provided and file exists -> File(path)
    /// 2. `cookies_browser_var`: if provided and valid supported browser -> Browser(browser)
    /// 3. `config_cookies_path`: if provided and file exists -> File(path)
    /// 4. `detected_browser`: if provided -> Browser(browser)
    /// 5. None (fallback unauthenticated mode)
    pub fn resolve_hierarchy<FExists>(
        cookies_file_var: Option<&str>,
        cookies_browser_var: Option<&str>,
        config_cookies_path: Option<std::path::PathBuf>,
        detected_browser: Option<Browser>,
        file_exists: FExists,
    ) -> Option<Self>
    where
        FExists: Fn(&std::path::Path) -> bool,
    {
        // 1. YTM_COOKIES_FILE
        if let Some(file) = cookies_file_var {
            let path = std::path::PathBuf::from(file.trim());
            if !path.as_os_str().is_empty() && file_exists(&path) {
                return Some(CookieSource::File(path));
            }
        }

        // 2. YTM_COOKIES_FROM_BROWSER
        if let Some(browser_str) = cookies_browser_var {
            if let Some(browser) = Browser::parse(browser_str) {
                return Some(CookieSource::Browser(browser));
            }
        }

        // 3. User configuration directory file
        if let Some(config_path) = config_cookies_path {
            if file_exists(&config_path) {
                return Some(CookieSource::File(config_path));
            }
        }

        // 4. Platform-aware auto-detection
        if let Some(browser) = detected_browser {
            return Some(CookieSource::Browser(browser));
        }

        // 5. Fallback unauthenticated mode
        None
    }

    /// Resolves cookie source from environment variables, user config directory, or platform defaults.
    pub fn resolve() -> Option<Self> {
        let file_var = std::env::var("YTM_COOKIES_FILE").ok();
        let browser_var = std::env::var("YTM_COOKIES_FROM_BROWSER").ok();
        let config_cookies = default_cookies_file();
        let detected = detect_platform_browser();

        Self::resolve_hierarchy(
            file_var.as_deref(),
            browser_var.as_deref(),
            config_cookies,
            detected,
            |p| p.is_file(),
        )
    }
}

/// Resolves user config directory given optional XDG_CONFIG_HOME and HOME paths.
pub fn default_config_dir_from(xdg: Option<&str>, home: Option<&str>) -> Option<std::path::PathBuf> {
    if let Some(xdg_val) = xdg {
        let trimmed = xdg_val.trim();
        if !trimmed.is_empty() {
            return Some(std::path::PathBuf::from(trimmed).join("ytm"));
        }
    }
    if let Some(home_val) = home {
        let trimmed = home_val.trim();
        if !trimmed.is_empty() {
            return Some(std::path::PathBuf::from(trimmed).join(".config").join("ytm"));
        }
    }
    None
}

/// Resolves user cookies file path given optional XDG_CONFIG_HOME and HOME paths.
pub fn default_cookies_file_from(xdg: Option<&str>, home: Option<&str>) -> Option<std::path::PathBuf> {
    default_config_dir_from(xdg, home).map(|dir| dir.join("cookies.txt"))
}

/// Resolves the default ytm user configuration directory using the current environment.
pub fn default_config_dir() -> Option<std::path::PathBuf> {
    let xdg = std::env::var("XDG_CONFIG_HOME").ok();
    let home = std::env::var("HOME").ok();
    default_config_dir_from(xdg.as_deref(), home.as_deref())
}

/// Resolves the default cookies.txt path in the user configuration directory.
pub fn default_cookies_file() -> Option<std::path::PathBuf> {
    default_config_dir().map(|dir| dir.join("cookies.txt"))
}

/// Helper to detect first existing browser from an ordered list of browser candidates and their potential paths.
pub fn detect_browser_from_candidates(
    candidates: &[(Browser, Vec<std::path::PathBuf>)],
) -> Option<Browser> {
    for (browser, paths) in candidates {
        for path in paths {
            if path.exists() {
                return Some(*browser);
            }
        }
    }
    None
}

/// Detects available browser on macOS given an optional HOME path.
pub fn detect_browser_macos(home: Option<&std::path::Path>) -> Option<Browser> {
    let mut candidates = Vec::new();

    // 1. First check user profile directories (where cookies actually live)
    if let Some(h) = home {
        let app_supp = h.join("Library/Application Support");
        candidates.push((Browser::Chrome, vec![app_supp.join("Google/Chrome")]));
        candidates.push((Browser::Brave, vec![app_supp.join("BraveSoftware/Brave-Browser")]));
        candidates.push((Browser::Firefox, vec![app_supp.join("Firefox")]));
        candidates.push((Browser::Edge, vec![app_supp.join("Microsoft Edge")]));
        candidates.push((Browser::Chromium, vec![app_supp.join("Chromium")]));
        candidates.push((Browser::Opera, vec![app_supp.join("com.operasoftware.Opera")]));
        candidates.push((Browser::Vivaldi, vec![app_supp.join("Vivaldi")]));
        candidates.push((Browser::Whale, vec![app_supp.join("Naver/Whale")]));
        candidates.push((
            Browser::Safari,
            vec![
                h.join("Library/Safari"),
                h.join("Library/Containers/com.apple.Safari"),
                h.join("Library/Cookies"),
            ],
        ));

        if let Some(found) = detect_browser_from_candidates(&candidates) {
            return Some(found);
        }
    }

    // 2. Fallback to checking installed application bundles
    let app_candidates = vec![
        (Browser::Chrome, vec![std::path::PathBuf::from("/Applications/Google Chrome.app")]),
        (Browser::Brave, vec![std::path::PathBuf::from("/Applications/Brave Browser.app")]),
        (Browser::Firefox, vec![std::path::PathBuf::from("/Applications/Firefox.app")]),
        (Browser::Edge, vec![std::path::PathBuf::from("/Applications/Microsoft Edge.app")]),
        (Browser::Chromium, vec![std::path::PathBuf::from("/Applications/Chromium.app")]),
        (Browser::Opera, vec![std::path::PathBuf::from("/Applications/Opera.app")]),
        (Browser::Vivaldi, vec![std::path::PathBuf::from("/Applications/Vivaldi.app")]),
        (Browser::Whale, vec![std::path::PathBuf::from("/Applications/Naver Whale.app")]),
        (
            Browser::Safari,
            vec![
                std::path::PathBuf::from("/Applications/Safari.app"),
                std::path::PathBuf::from("/System/Applications/Safari.app"),
            ],
        ),
    ];

    detect_browser_from_candidates(&app_candidates)
}

/// Detects available browser on Linux given an optional HOME path.
pub fn detect_browser_linux(home: Option<&std::path::Path>) -> Option<Browser> {
    let home = home?;
    let config_dir = home.join(".config");

    let candidates = vec![
        (Browser::Chrome, vec![config_dir.join("google-chrome")]),
        (Browser::Chromium, vec![config_dir.join("chromium")]),
        (Browser::Brave, vec![config_dir.join("BraveSoftware/Brave-Browser")]),
        (Browser::Firefox, vec![home.join(".mozilla/firefox")]),
        (Browser::Edge, vec![config_dir.join("microsoft-edge")]),
        (Browser::Opera, vec![config_dir.join("opera")]),
        (Browser::Vivaldi, vec![config_dir.join("vivaldi")]),
        (Browser::Whale, vec![config_dir.join("naver-whale")]),
    ];

    detect_browser_from_candidates(&candidates)
}

/// Detects an available browser according to the host platform.
pub fn detect_platform_browser() -> Option<Browser> {
    let home = std::env::var("HOME").ok().map(std::path::PathBuf::from);
    #[cfg(target_os = "macos")]
    {
        detect_browser_macos(home.as_deref())
    }
    #[cfg(target_os = "linux")]
    {
        detect_browser_linux(home.as_deref())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = home;
        None
    }
}

/// Builds a `yt-dlp` Command with standard flags (`--no-warnings`) and cookies from the provided CookieSource.
pub fn build_ytdlp_command_with_cookies(cookie_source: Option<&CookieSource>) -> std::process::Command {
    let mut cmd = std::process::Command::new("yt-dlp");
    cmd.arg("--no-warnings");
    if let Some(cookies) = cookie_source {
        cmd.args(cookies.ytdlp_args());
    }
    cmd
}

/// Builds a `yt-dlp` Command with standard flags (`--no-warnings`) and resolved cookies from CookieSource::resolve().
pub fn build_ytdlp_command() -> std::process::Command {
    let cookies = CookieSource::resolve();
    build_ytdlp_command_with_cookies(cookies.as_ref())
}

/// Configures an `mpv` Command with cookies from the provided CookieSource.
pub fn configure_mpv_command_with_cookies<'a>(
    cmd: &'a mut std::process::Command,
    cookie_source: Option<&CookieSource>,
) -> &'a mut std::process::Command {
    if let Some(cookies) = cookie_source {
        cmd.arg(cookies.mpv_arg());
    }
    cmd
}

/// Configures an `mpv` Command with cookies resolved from CookieSource::resolve().
pub fn configure_mpv_command(cmd: &mut std::process::Command) -> &mut std::process::Command {
    let cookies = CookieSource::resolve();
    configure_mpv_command_with_cookies(cmd, cookies.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_browser_parsing_and_as_str() {
        assert_eq!(Browser::parse("safari"), Some(Browser::Safari));
        assert_eq!(Browser::parse("SAFARI"), Some(Browser::Safari));
        assert_eq!(Browser::parse("  firefox  "), Some(Browser::Firefox));
        assert_eq!(Browser::parse("chrome"), Some(Browser::Chrome));
        assert_eq!(Browser::parse("brave"), Some(Browser::Brave));
        assert_eq!(Browser::parse("edge"), Some(Browser::Edge));
        assert_eq!(Browser::parse("chromium"), Some(Browser::Chromium));
        assert_eq!(Browser::parse("opera"), Some(Browser::Opera));
        assert_eq!(Browser::parse("vivaldi"), Some(Browser::Vivaldi));
        assert_eq!(Browser::parse("whale"), Some(Browser::Whale));

        assert_eq!(Browser::parse("invalid"), None);
        assert_eq!(Browser::parse(""), None);

        assert_eq!(Browser::Safari.as_str(), "safari");
        assert_eq!(Browser::Firefox.as_str(), "firefox");
        assert_eq!(Browser::Chrome.as_str(), "chrome");
        assert_eq!(Browser::Brave.as_str(), "brave");
        assert_eq!(Browser::Edge.as_str(), "edge");
        assert_eq!(Browser::Chromium.as_str(), "chromium");
        assert_eq!(Browser::Opera.as_str(), "opera");
        assert_eq!(Browser::Vivaldi.as_str(), "vivaldi");
        assert_eq!(Browser::Whale.as_str(), "whale");

        assert_eq!(Browser::Firefox.to_string(), "firefox");
        assert_eq!("chrome".parse::<Browser>(), Ok(Browser::Chrome));
        assert!("unknown".parse::<Browser>().is_err());
    }

    #[test]
    fn test_cookie_source_formatting_and_args() {
        let browser_cookie = CookieSource::Browser(Browser::Firefox);
        assert_eq!(
            browser_cookie.ytdlp_args(),
            vec!["--cookies-from-browser", "firefox"]
        );
        assert_eq!(
            browser_cookie.mpv_arg(),
            "--ytdl-raw-options=cookies-from-browser=firefox"
        );

        let file_cookie = CookieSource::File(std::path::PathBuf::from("/etc/cookies.txt"));
        assert_eq!(
            file_cookie.ytdlp_args(),
            vec!["--cookies", "/etc/cookies.txt"]
        );
        assert_eq!(
            file_cookie.mpv_arg(),
            "--ytdl-raw-options=cookies=/etc/cookies.txt"
        );
    }

    #[test]
    fn test_config_and_cookies_file_resolution() {
        use std::path::PathBuf;

        // 1. Both XDG_CONFIG_HOME and HOME set: XDG takes precedence
        assert_eq!(
            default_config_dir_from(Some("/custom/xdg"), Some("/home/user")),
            Some(PathBuf::from("/custom/xdg/ytm"))
        );
        assert_eq!(
            default_cookies_file_from(Some("/custom/xdg"), Some("/home/user")),
            Some(PathBuf::from("/custom/xdg/ytm/cookies.txt"))
        );

        // 2. Only HOME set: fallback to ~/.config/ytm
        assert_eq!(
            default_config_dir_from(None, Some("/home/user")),
            Some(PathBuf::from("/home/user/.config/ytm"))
        );
        assert_eq!(
            default_cookies_file_from(None, Some("/home/user")),
            Some(PathBuf::from("/home/user/.config/ytm/cookies.txt"))
        );

        // 3. XDG is empty string: fallback to HOME
        assert_eq!(
            default_config_dir_from(Some("   "), Some("/home/user")),
            Some(PathBuf::from("/home/user/.config/ytm"))
        );

        // 4. Neither set
        assert_eq!(default_config_dir_from(None, None), None);
        assert_eq!(default_cookies_file_from(None, None), None);
    }

    #[test]
    fn test_detect_browser_platform() {
        let temp_home = std::env::temp_dir().join(format!("ytm_test_browser_detect_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_home);
        std::fs::create_dir_all(&temp_home).unwrap();

        // 1. Empty home: macos detects None if /Applications are not mocked or if we check user paths
        // When Firefox profile exists on macOS
        let ff_profile = temp_home.join("Library/Application Support/Firefox");
        std::fs::create_dir_all(&ff_profile).unwrap();
        assert_eq!(detect_browser_macos(Some(&temp_home)), Some(Browser::Firefox));

        // When Chrome profile is also added, Chrome has higher precedence than Firefox
        let chrome_profile = temp_home.join("Library/Application Support/Google/Chrome");
        std::fs::create_dir_all(&chrome_profile).unwrap();
        assert_eq!(detect_browser_macos(Some(&temp_home)), Some(Browser::Chrome));

        // On Linux: test with ~/.config/google-chrome and ~/.mozilla/firefox
        let linux_home = std::env::temp_dir().join(format!("ytm_test_linux_detect_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&linux_home);
        std::fs::create_dir_all(&linux_home).unwrap();

        assert_eq!(detect_browser_linux(Some(&linux_home)), None);

        let linux_ff = linux_home.join(".mozilla/firefox");
        std::fs::create_dir_all(&linux_ff).unwrap();
        assert_eq!(detect_browser_linux(Some(&linux_home)), Some(Browser::Firefox));

        let linux_brave = linux_home.join(".config/BraveSoftware/Brave-Browser");
        std::fs::create_dir_all(&linux_brave).unwrap();
        // Brave has higher precedence than Firefox
        assert_eq!(detect_browser_linux(Some(&linux_home)), Some(Browser::Brave));

        let _ = std::fs::remove_dir_all(&temp_home);
        let _ = std::fs::remove_dir_all(&linux_home);
    }

    #[test]
    fn test_resolution_hierarchy() {
        use std::path::PathBuf;

        let file_path = PathBuf::from("/custom/cookies.txt");
        let config_path = PathBuf::from("/home/user/.config/ytm/cookies.txt");

        // 1. YTM_COOKIES_FILE exists: takes top precedence over everything
        let res1 = CookieSource::resolve_hierarchy(
            Some("/custom/cookies.txt"),
            Some("chrome"),
            Some(config_path.clone()),
            Some(Browser::Firefox),
            |p| p == file_path.as_path(),
        );
        assert_eq!(res1, Some(CookieSource::File(file_path.clone())));

        // 2. YTM_COOKIES_FILE non-existent: falls through to YTM_COOKIES_FROM_BROWSER
        let res2 = CookieSource::resolve_hierarchy(
            Some("/nonexistent/cookies.txt"),
            Some("chrome"),
            Some(config_path.clone()),
            Some(Browser::Firefox),
            |p| p == file_path.as_path(),
        );
        assert_eq!(res2, Some(CookieSource::Browser(Browser::Chrome)));

        // 3. YTM_COOKIES_FROM_BROWSER invalid: falls through to user config cookies file
        let res3 = CookieSource::resolve_hierarchy(
            None,
            Some("invalid_browser_name"),
            Some(config_path.clone()),
            Some(Browser::Firefox),
            |p| p == config_path.as_path(),
        );
        assert_eq!(res3, Some(CookieSource::File(config_path.clone())));

        // 4. Config cookies file does not exist: falls through to platform auto-detection
        let res4 = CookieSource::resolve_hierarchy(
            None,
            None,
            Some(config_path.clone()),
            Some(Browser::Firefox),
            |_| false,
        );
        assert_eq!(res4, Some(CookieSource::Browser(Browser::Firefox)));

        // 5. Fallback unauthenticated mode: nothing available -> None
        let res5 = CookieSource::resolve_hierarchy(
            None,
            None,
            Some(config_path.clone()),
            None,
            |_| false,
        );
        assert_eq!(res5, None);
    }

    #[test]
    fn test_command_builders() {
        use std::ffi::OsStr;
        use std::path::PathBuf;
        use std::process::Command;

        // 1. Browser cookies
        let browser_source = CookieSource::Browser(Browser::Chrome);
        let cmd = build_ytdlp_command_with_cookies(Some(&browser_source));
        assert_eq!(cmd.get_program(), "yt-dlp");
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(args, vec!["--no-warnings", "--cookies-from-browser", "chrome"]);

        let mut mpv_cmd = Command::new("mpv");
        configure_mpv_command_with_cookies(&mut mpv_cmd, Some(&browser_source));
        let mpv_args: Vec<&OsStr> = mpv_cmd.get_args().collect();
        assert_eq!(mpv_args, vec!["--ytdl-raw-options=cookies-from-browser=chrome"]);

        // 2. File cookies
        let file_source = CookieSource::File(PathBuf::from("/path/to/cookies.txt"));
        let cmd2 = build_ytdlp_command_with_cookies(Some(&file_source));
        let args2: Vec<&OsStr> = cmd2.get_args().collect();
        assert_eq!(args2, vec!["--no-warnings", "--cookies", "/path/to/cookies.txt"]);

        let mut mpv_cmd2 = Command::new("mpv");
        configure_mpv_command_with_cookies(&mut mpv_cmd2, Some(&file_source));
        let mpv_args2: Vec<&OsStr> = mpv_cmd2.get_args().collect();
        assert_eq!(mpv_args2, vec!["--ytdl-raw-options=cookies=/path/to/cookies.txt"]);

        // 3. No cookies (fallback)
        let cmd3 = build_ytdlp_command_with_cookies(None);
        let args3: Vec<&OsStr> = cmd3.get_args().collect();
        assert_eq!(args3, vec!["--no-warnings"]);

        let mut mpv_cmd3 = Command::new("mpv");
        configure_mpv_command_with_cookies(&mut mpv_cmd3, None);
        let mpv_args3: Vec<&OsStr> = mpv_cmd3.get_args().collect();
        assert!(mpv_args3.is_empty());
    }
}

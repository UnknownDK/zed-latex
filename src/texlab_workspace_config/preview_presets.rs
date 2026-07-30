//! Stores information specific to each of the PDF previewers supported by this Zed extension.
//! That is, how to detect them, and what command to use for forward search (and inverse when possible).
//!
//! The module primarily:
//! - Defines supported PDF previewers
//! - Creates appropriate `texlab.settings.forwardSearch` settings for each previewer
//! - Detects an available previewer in the system

use super::types::TexlabForwardSearchSettings;
use crate::zed_command::CommandName;
use zed_extension_api as zed;

/// Represents different types of PDF preview applications that this Zed extension supports for previewing built LaTeX documents.
#[allow(dead_code)]
pub enum Preview {
    /// PDF viewer popular in minimalistic linux installs
    Zathura,
    /// Recommended PDF viewer for macOS
    Skim,
    Sioyek,
    QPDFView,
    /// KDE document viewer
    Okular,
    /// PDF viewer for Windows
    SumatraPDF {
        path: String,
    },
    /// GNOME document viewer
    Evince {
        evince_synctex_path: String,
    },
}

impl Preview {
    /// Creates the appropriate `texlab.settings.forwardSearch` settings for the specific PDF previewer.
    ///
    /// This function configures the executable path and command line arguments needed for
    /// synctex-based forward searching (and inverse when possible) in each supported previewer.
    ///
    /// # Arguments
    ///
    /// * `zed_command` - The Zed editor command to use for inverse search (opening files from the PDF viewer)
    ///
    /// # Returns
    ///
    /// `TexlabForwardSearchSettings` containing the executable and arguments for forward search
    pub fn create_preset(&self, zed_command: CommandName) -> TexlabForwardSearchSettings {
        match self {
            Preview::Zathura => TexlabForwardSearchSettings {
                executable: Some("zathura".to_string()),
                args: Some(vec![
                    "--synctex-forward".to_string(),
                    "%l:1:%f".to_string(),
                    "-x".to_string(),
                    format!("{} {}", zed_command.to_str(), "%%{input}:%%{line}"),
                    "%p".to_string(),
                ]),
            },
            Preview::Skim => TexlabForwardSearchSettings {
                executable: Some(
                    "/Applications/Skim.app/Contents/SharedSupport/displayline".to_string(),
                ),
                args: Some(vec![
                    "-r".to_string(),
                    "%l".to_string(),
                    "%p".to_string(),
                    "%f".to_string(),
                ]),
            },
            Preview::Sioyek => TexlabForwardSearchSettings {
                executable: Some("sioyek".to_string()),
                args: Some(vec![
                    "--reuse-window".to_string(),
                    "--inverse-search".to_string(),
                    format!("{} \"%%1\":%%2", zed_command.to_str()),
                    "--forward-search-file".to_string(),
                    "%f".to_string(),
                    "--forward-search-line".to_string(),
                    "%l".to_string(),
                    "%p".to_string(),
                ]),
            },
            Preview::Okular => TexlabForwardSearchSettings {
                // Unfortunately, there is no single okular command that can be used for the
                // forward search command in a way that sets up the inverse search command.
                // Therefore, we resort to a shell command involving two okular commands.
                //
                // This shell command attempts to open okular performing a forward search and
                // setting the inverse-search command to open the file in zed at the correct
                // location.
                // However the `--unique` flag conflicts with the `--editor-cmd` flag, but
                // only if okular is already open. At that point, the same command is run
                // again but without the `--editor-cmd` flag, which is ok because the editor
                // command (inverse search) would already be set at that point.
                executable: Some("sh".to_string()),
                args: Some(vec![
                    "-c".to_string(),
                    format!(
                        "okular --unique --noraise --editor-cmd \"{} '%%f':%%l:%%c\" \"%p#src:%l %f\" || okular --unique --noraise \"%p#src:%l %f\"",
                        zed_command.to_str()
                    ),
                ]),
            },
            Preview::QPDFView => TexlabForwardSearchSettings {
                executable: Some("qpdfview".to_string()),
                args: Some(vec!["--unique".to_string(), "%p#src:%f:%l:1".to_string()]),
            },
            Preview::Evince{ ref evince_synctex_path} => TexlabForwardSearchSettings {
                executable: Some("python3".to_string()),
                args: Some(vec![
                    evince_synctex_path.clone(),
                    "-f".to_string(),
                    "%l".to_string(),
                    "-t".to_string(),
                    "%f".to_string(),
                    "%p".to_string(),
                    format!("{} %%f:%%l", zed_command.to_str())
                ]),
            },
            Preview::SumatraPDF{ ref path } => TexlabForwardSearchSettings {
                executable: Some(path.clone()),
                args: Some(vec![
                    "-reuse-instance".to_string(),
                    "%p".to_string(),
                    "-forward-search".to_string(),
                    "%f".to_string(),
                    "%l".to_string()
                ]),
            },
        }
    }

    /// Detects a PDF previewer available on the system.
    ///
    /// This function checks for the availability of various PDF previewers by looking for
    /// their executables in the system PATH. For Evince, it also downloads the pinned
    /// `evince_synctex.py` helper (run via the system `python3`) used for synctex
    /// forward/inverse search.
    ///
    /// # Arguments
    ///
    /// * `worktree` - Reference to the Zed worktree, used for checking executable availability
    ///
    /// # Returns
    ///
    /// `Option<Preview>` containing the first supported PDF previewer found, or `None` if no
    /// supported previewer is available
    pub fn determine(worktree: &zed::Worktree) -> Option<Preview> {
        let (platform, _) = zed::current_platform();

        if platform == zed::Os::Mac {
            if worktree
                .which("/Applications/Skim.app/Contents/SharedSupport/displayline")
                .is_some()
            {
                return Some(Preview::Skim);
            }
        }

        if platform == zed::Os::Windows {
            let localappdata = worktree
                .shell_env()
                .iter()
                .find(|&var| var.0 == "LOCALAPPDATA")?
                .1
                .clone();
            let potential_sumatra_path = format!("{localappdata}\\SumatraPDF\\SumatraPDF.exe");
            if worktree.which(&potential_sumatra_path).is_some() {
                return Some(Preview::SumatraPDF {
                    path: potential_sumatra_path,
                });
            }
        }

        if worktree.which("evince").is_some() {
            // Download the `evince_synctex.py` helper (used for synctex
            // forward/inverse search) and run it with the system `python3`,
            // which provides the required `gi` and `dbus` modules. It is
            // pinned to a specific commit of the fork below; the commit hash is
            // part of the cached filename, so bumping COMMIT_HASH transparently
            // triggers a fresh download.
            const GITHUB_REPO_NAME: &str = "UnknownDK/evince-synctex";
            const COMMIT_HASH: &str = "511fd2ef6862b43d1565ba2efa4b8da243bff17b";
            let script_name = format!("evince_synctex_{}.py", &COMMIT_HASH[..12]);

            // The following would all be useless if the string path for the
            // script in CWD cannot be obtained:
            if let Some(evince_synctex_path) = (|| {
                Some(format!(
                    "{}/{script_name}",
                    std::env::current_dir().ok()?.as_os_str().to_str()?
                ))
            })() {
                // Reuse a previously downloaded copy for this commit if present.
                if std::fs::metadata(&script_name).map_or(false, |stat| stat.is_file()) {
                    return Some(Preview::Evince { evince_synctex_path });
                }
                // Otherwise choose evince for preview, provided the helper
                // downloads successfully.
                if zed::download_file(
                    format!("https://raw.githubusercontent.com/{GITHUB_REPO_NAME}/{COMMIT_HASH}/evince_synctex.py").as_str(),
                    &script_name,
                    zed::DownloadedFileType::Uncompressed
                ).is_ok() {
                    return Some(Preview::Evince { evince_synctex_path });
                }
            }
        }
        if worktree.which("zathura").is_some() {
            return Some(Preview::Zathura);
        }
        if worktree.which("sioyek").is_some() {
            return Some(Preview::Sioyek);
        }
        if worktree.which("qpdfview").is_some() {
            return Some(Preview::QPDFView);
        }
        if worktree.which("okular").is_some() {
            return Some(Preview::Okular);
        }

        None
    }
}

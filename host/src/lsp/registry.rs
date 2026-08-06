use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerSpec {
    pub name: &'static str,
    pub language_id: &'static str,
    pub argv: Vec<PathBuf>,
    pub args: Vec<String>,
    pub root_markers: &'static [&'static str],
}

impl ServerSpec {
    pub fn missing_message(&self) -> String {
        format!(
            "LSP server '{}' not found at {}",
            self.name,
            self.argv[0].display()
        )
    }
}

pub fn servers_for_path(path: &Path) -> Vec<ServerSpec> {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return Vec::new();
    };
    match ext {
        "rs" => vec![server("rust_analyzer")],
        "py" => vec![server("pyright")],
        "ts" | "tsx" => vec![server("ts_ls"), server("eslint")],
        "js" | "jsx" | "mjs" | "cjs" => vec![server("ts_ls"), server("eslint")],
        "lua" => vec![server("lua_ls")],
        _ => Vec::new(),
    }
}

pub fn server(name: &str) -> ServerSpec {
    let mason = mason_bin();
    match name {
        "rust_analyzer" => ServerSpec {
            name: "rust_analyzer",
            language_id: "rust",
            argv: vec![mason.join("rust-analyzer")],
            args: vec![],
            root_markers: &["Cargo.toml", "rust-project.json", ".git"],
        },
        "pyright" => ServerSpec {
            name: "pyright",
            language_id: "python",
            argv: vec![mason.join("pyright-langserver")],
            args: vec!["--stdio".into()],
            root_markers: &[
                "pyproject.toml",
                "setup.py",
                "setup.cfg",
                "requirements.txt",
                ".git",
            ],
        },
        "ts_ls" => ServerSpec {
            name: "ts_ls",
            language_id: "typescript",
            argv: vec![mason.join("typescript-language-server")],
            args: vec!["--stdio".into()],
            root_markers: &["package.json", "tsconfig.json", "jsconfig.json", ".git"],
        },
        "eslint" => ServerSpec {
            name: "eslint",
            language_id: "javascript",
            argv: vec![mason.join("vscode-eslint-language-server")],
            args: vec!["--stdio".into()],
            root_markers: &[
                "eslint.config.js",
                ".eslintrc",
                ".eslintrc.js",
                ".eslintrc.json",
                "package.json",
                ".git",
            ],
        },
        "lua_ls" => ServerSpec {
            name: "lua_ls",
            language_id: "lua",
            argv: vec![mason.join("lua-language-server")],
            args: vec![],
            root_markers: &[".luarc.json", ".luarc.jsonc", ".stylua.toml", ".git"],
        },
        _ => panic!("unknown LSP server {name}"),
    }
}

pub fn root_for(path: &Path, markers: &[&str]) -> PathBuf {
    let mut dir = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    loop {
        if markers.iter().any(|marker| dir.join(marker).exists()) {
            return dir;
        }
        if !dir.pop() {
            return path.parent().unwrap_or(Path::new(".")).to_path_buf();
        }
    }
}

fn mason_bin() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(""));
    home.join(".local/share/nvim/mason/bin")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_servers_use_mason_bin_before_path_lookup() {
        let rust = server("rust_analyzer");
        assert!(rust.argv[0]
            .to_string_lossy()
            .contains(".local/share/nvim/mason/bin"));
        assert!(rust.argv[0].ends_with("rust-analyzer"));
        assert!(rust.args.is_empty());

        let pyright = server("pyright");
        assert_eq!(pyright.args, vec!["--stdio"]);
    }

    #[test]
    fn file_extensions_choose_the_named_servers() {
        let names: Vec<_> = servers_for_path(Path::new("x.ts"))
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, vec!["ts_ls", "eslint"]);
        assert_eq!(servers_for_path(Path::new("x.md")).len(), 0);
    }
}

//! The owner's mappings, as a keymap the editor consults.
//!
//! This is the half of `pin keymap_preservation` that makes the other half
//! true: [`crate::luaconf`] reads the real `init.lua`, and this turns what it
//! read into behaviour. Nothing here knows which mappings the owner has — it
//! resolves whatever the file contained.
//!
//! Matching is vim's: keep the keys typed so far, and while they are a *prefix*
//! of some left-hand side, wait. `s` and `ss` are both mapped, so pressing `s`
//! cannot act until the next key says which one was meant. Where vim breaks the
//! tie with `timeoutlen`, this waits for the next key instead — there is no
//! clock in the input path, and a wrong guess is worse than a wait.

use std::collections::BTreeMap;

use crate::core::key::{parse, Key};
use crate::luaconf::{Mapping, NvimConfig};

#[derive(Clone, Debug, PartialEq)]
pub enum Rhs {
    /// Keys to replay, e.g. `Y -> y$`.
    Keys(Vec<Key>),
    /// An Ex command or a `<cmd>…<CR>`, e.g. `<space>o -> Telescope find_files`.
    Command(String),
    /// Mapped to nothing on purpose (`<NOP>`).
    Nothing,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Match<'a> {
    /// The keys so far are the whole of a left-hand side.
    Exact(&'a Rhs),
    /// They are the start of one, so nothing may happen yet.
    Prefix,
    /// No mapping can begin with these keys.
    None,
}

#[derive(Default, Debug)]
pub struct Keymap {
    /// Per Neovim mode letter, longest left-hand side first.
    by_mode: BTreeMap<char, Vec<(Vec<Key>, Rhs)>>,
    /// Commands that were mapped but cannot be carried out here, kept so the
    /// editor can say so instead of appearing to do nothing.
    pub unsupported: Vec<String>,
}

impl Keymap {
    pub fn from_config(config: &NvimConfig) -> Self {
        let leader = config.leader();
        let mut by_mode: BTreeMap<char, Vec<(Vec<Key>, Rhs)>> = BTreeMap::new();
        for mapping in &config.mappings {
            let lhs = parse(&expand_leader(&mapping.lhs, &leader));
            if lhs.is_empty() {
                continue;
            }
            let rhs = right_hand_side(mapping);
            for mode in mapping.mode.chars() {
                // `x` is visual in Neovim's own naming, and `v` covers visual
                // and select; both land on this editor's visual mode.
                let mode = if mode == 'x' { 'v' } else { mode };
                let slot = by_mode.entry(mode).or_default();
                // A later mapping of the same keys wins, as in Neovim: the
                // config sets `<leader>h` twice and means the second.
                slot.retain(|(existing, _)| existing != &lhs);
                slot.push((lhs.clone(), rhs.clone()));
            }
        }
        for slot in by_mode.values_mut() {
            slot.sort_by_key(|(lhs, _)| std::cmp::Reverse(lhs.len()));
        }
        Self {
            by_mode,
            unsupported: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.by_mode.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How the keys typed so far relate to this mode's mappings.
    pub fn lookup(&self, mode: char, keys: &[Key]) -> Match<'_> {
        let Some(slot) = self.by_mode.get(&mode) else {
            return Match::None;
        };
        if let Some((_, rhs)) = slot.iter().find(|(lhs, _)| lhs.as_slice() == keys) {
            // An exact hit still yields to a longer mapping that starts the same
            // way: `<Space>` is mapped to <NOP> and `<Space>o` opens the
            // picker, and firing the shorter one would make the longer
            // unreachable.
            if slot
                .iter()
                .any(|(lhs, _)| lhs.len() > keys.len() && lhs.starts_with(keys))
            {
                return Match::Prefix;
            }
            return Match::Exact(rhs);
        }
        if slot
            .iter()
            .any(|(lhs, _)| lhs.len() > keys.len() && lhs.starts_with(keys))
        {
            return Match::Prefix;
        }
        Match::None
    }
}

/// `<Leader>q` is `<Space>q` once the config has said what the leader is.
fn expand_leader(lhs: &str, leader: &str) -> String {
    let mut out = String::with_capacity(lhs.len());
    let mut rest = lhs;
    while let Some(at) = rest.to_ascii_lowercase().find("<leader>") {
        out.push_str(&rest[..at]);
        out.push_str(leader);
        rest = &rest[at + "<leader>".len()..];
    }
    out.push_str(rest);
    out
}

fn right_hand_side(mapping: &Mapping) -> Rhs {
    if mapping.rhs.eq_ignore_ascii_case("<nop>") {
        return Rhs::Nothing;
    }
    match mapping.command() {
        Some(command) => Rhs::Command(command.to_string()),
        None => Rhs::Keys(parse(&mapping.rhs)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::key::{Code, Named};
    use crate::luaconf::Mapping;

    fn config(mappings: &[(&str, &str, &str)]) -> NvimConfig {
        let mut config = NvimConfig::default();
        config.globals.insert(
            "mapleader".into(),
            crate::luaconf::Setting::Text(" ".into()),
        );
        for (mode, lhs, rhs) in mappings {
            config.mappings.push(Mapping {
                mode: mode.to_string(),
                lhs: lhs.to_string(),
                rhs: rhs.to_string(),
            });
        }
        config
    }

    #[test]
    fn keys_and_commands_are_told_apart() {
        let keymap = Keymap::from_config(&config(&[
            ("n", "Y", "y$"),
            ("n", "<space>o", "<cmd>Telescope find_files<cr>"),
        ]));
        assert_eq!(
            keymap.lookup('n', &parse("Y")),
            Match::Exact(&Rhs::Keys(parse("y$")))
        );
        assert_eq!(
            keymap.lookup('n', &parse(" o")),
            Match::Exact(&Rhs::Command("Telescope find_files".into()))
        );
    }

    #[test]
    fn a_shorter_mapping_waits_for_a_longer_one_that_starts_the_same_way() {
        // The config maps <Space> to <NOP> and <Space>o to the picker. Firing
        // the shorter one on sight makes the longer unreachable.
        let keymap = Keymap::from_config(&config(&[
            ("n", "<Space>", "<NOP>"),
            ("n", "<space>o", "<cmd>Telescope find_files<cr>"),
        ]));
        assert_eq!(keymap.lookup('n', &parse(" ")), Match::Prefix);
        assert!(matches!(
            keymap.lookup('n', &parse(" o")),
            Match::Exact(Rhs::Command(_))
        ));
        assert_eq!(keymap.lookup('n', &parse(" z")), Match::None);
    }

    #[test]
    fn s_and_ss_both_exist_so_s_alone_has_to_wait() {
        let keymap = Keymap::from_config(&config(&[
            ("n", "s", "<cmd>lua require(\"substitute\").eol()<CR>"),
            ("n", "ss", "<cmd>lua require(\"substitute\").line()<CR>"),
        ]));
        assert_eq!(keymap.lookup('n', &parse("s")), Match::Prefix);
        assert!(matches!(keymap.lookup('n', &parse("ss")), Match::Exact(_)));
    }

    #[test]
    fn the_leader_is_expanded_from_the_config_not_assumed() {
        let keymap = Keymap::from_config(&config(&[("n", "<Leader>q", "<cmd>q<CR>")]));
        assert!(matches!(
            keymap.lookup('n', &parse(" q")),
            Match::Exact(Rhs::Command(_))
        ));

        let mut backslash = config(&[("n", "<Leader>q", "<cmd>q<CR>")]);
        backslash.globals.clear();
        let keymap = Keymap::from_config(&backslash);
        assert!(matches!(
            keymap.lookup('n', &parse("\\q")),
            Match::Exact(Rhs::Command(_))
        ));
    }

    #[test]
    fn x_mode_lands_on_visual() {
        let keymap = Keymap::from_config(&config(&[("x", "S", "<cmd>lua sub()<CR>")]));
        assert!(matches!(keymap.lookup('v', &parse("S")), Match::Exact(_)));
    }

    #[test]
    fn a_repeated_left_hand_side_takes_the_last_one() {
        // The config maps <leader>h twice and means the second.
        let keymap = Keymap::from_config(&config(&[
            ("n", "<Leader>h", "<cmd>BufferPrevious<CR>"),
            ("n", "<leader>h", "<cmd>Telescope neoclip plus<CR>"),
        ]));
        assert_eq!(
            keymap.lookup('n', &parse(" h")),
            Match::Exact(&Rhs::Command("Telescope neoclip plus".into()))
        );
    }

    #[test]
    fn nop_is_a_mapping_to_nothing_rather_than_an_absent_one() {
        let keymap = Keymap::from_config(&config(&[("n", "<Space>", "<NOP>")]));
        assert_eq!(keymap.lookup('n', &parse(" ")), Match::Exact(&Rhs::Nothing));
    }

    #[test]
    fn a_named_key_on_the_left_is_one_key() {
        let keymap = Keymap::from_config(&config(&[("n", "<tab>h", "<C-w>h")]));
        assert_eq!(keymap.lookup('n', &[Key::named(Named::Tab)]), Match::Prefix);
        let keys = [Key::named(Named::Tab), Key::char('h')];
        assert!(matches!(keymap.lookup('n', &keys), Match::Exact(_)));
    }

    #[test]
    fn f_is_a_prefix_only_for_the_keys_the_config_bound() {
        // fj/fk/fh/fl are mapped; f followed by anything else must fall through
        // to the built-in find-character.
        let keymap = Keymap::from_config(&config(&[("n", "fj", "15j"), ("n", "fk", "15k")]));
        assert_eq!(keymap.lookup('n', &parse("f")), Match::Prefix);
        assert_eq!(keymap.lookup('n', &parse("fx")), Match::None);
        assert_eq!(
            keymap.lookup('n', &parse("fj")),
            Match::Exact(&Rhs::Keys(vec![
                Key {
                    code: Code::Char('1'),
                    ctrl: false,
                    alt: false
                },
                Key {
                    code: Code::Char('5'),
                    ctrl: false,
                    alt: false
                },
                Key {
                    code: Code::Char('j'),
                    ctrl: false,
                    alt: false
                },
            ]))
        );
    }

    /// The two bindings that could not arrive at all before F-keys and ctrl
    /// case-folding existed.
    ///
    /// `<F5>` used to split into `<`, `F`, `5`, `>` and `<c-S>` used to be a
    /// different key from `<C-s>`, so `pin keymap_preservation` was false for
    /// these two however good the mapping engine was. Reading them out of the
    /// owner's real file is what says otherwise.
    #[test]
    fn the_owners_f_key_and_ctrl_bindings_are_reachable() {
        let config = crate::luaconf::load_default();
        if config.mappings.is_empty() {
            return;
        }
        let keymap = Keymap::from_config(&config);
        for keys in ["<F5>", "<c-S>", "<C-s>"] {
            let parsed = parse(keys);
            assert_eq!(parsed.len(), 1, "{keys} did not parse as one key: {parsed:?}");
            assert!(
                matches!(keymap.lookup('n', &parsed), Match::Exact(Rhs::Command(_))),
                "{keys} reaches no command in the owner's config",
            );
        }
        // `<c-S>` and `<C-s>` are the same key, so they must resolve alike.
        assert_eq!(keymap.lookup('n', &parse("<c-S>")), keymap.lookup('n', &parse("<C-s>")));
    }

    /// The owner's own file, when it is there.
    #[test]
    fn the_real_config_produces_a_keymap() {
        let config = crate::luaconf::load_default();
        if config.mappings.is_empty() {
            return;
        }
        let keymap = Keymap::from_config(&config);
        assert!(keymap.len() > 10, "only {} mappings", keymap.len());
        // The three the owner would notice first.
        assert!(matches!(
            keymap.lookup('i', &parse("kj")),
            Match::Exact(Rhs::Keys(_))
        ));
        assert!(matches!(
            keymap.lookup('n', &parse("H")),
            Match::Exact(Rhs::Keys(_))
        ));
        assert!(matches!(
            keymap.lookup('n', &parse(" o")),
            Match::Exact(Rhs::Command(_))
        ));
    }

    /// The owner's own file, when it is there, binds these keys today.
    #[test]
    fn the_real_config_keeps_f5_and_ctrl_s_reachable() {
        let config = crate::luaconf::load_default();
        if config.path.is_none() {
            return;
        }
        let keymap = Keymap::from_config(&config);
        let f5 = parse("<F5>");
        assert_eq!(f5.len(), 1);
        assert!(
            matches!(keymap.lookup('n', &f5), Match::Exact(Rhs::Command(command)) if command == "Jaq")
        );

        let ctrl_s = parse("<c-S>");
        assert_eq!(ctrl_s.len(), 1);
        assert_eq!(ctrl_s, parse("<C-s>"));
        assert!(
            matches!(keymap.lookup('n', &ctrl_s), Match::Exact(Rhs::Command(command)) if command == "FzfLua live_grep")
        );
    }
}

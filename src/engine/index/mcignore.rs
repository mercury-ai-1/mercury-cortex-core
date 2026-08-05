//! Compiled `.mcignore` pattern set used to exclude paths from indexing.

use std::io::BufRead;
use std::path::Path;
use std::sync::Arc;

use crate::engine::error::EngineError;

/// Compiled set of `.mcignore` patterns.
///
/// Supports:
/// - `#` comments and blank lines
/// - Trailing `/` for directory-only patterns
/// - `*` (any characters except `/`) and `**` (any characters including `/`)
/// - Leading `!` for negation (applied — gitignore last-match-wins semantics)
#[derive(Clone, Debug)]
pub struct McIgnore {
    patterns: Arc<Vec<McIgnorePattern>>,
}

#[derive(Clone, Debug)]
struct McIgnorePattern {
    pattern: String,
    is_negation: bool,
    dir_only: bool,
}

impl McIgnore {
    /// Load and compile patterns from a `.mcignore`-style file.
    ///
    /// Returns an empty set when the file does not exist (common before
    /// the user has customised their ignore list).
    pub fn load(path: &Path) -> Result<Self, EngineError> {
        let file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    patterns: Arc::new(Vec::new()),
                });
            }
            Err(e) => return Err(EngineError::Io(e)),
        };

        let mut patterns = Vec::new();
        for line in std::io::BufReader::new(file).lines() {
            let line = line.map_err(EngineError::Io)?;
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let is_negation = trimmed.starts_with('!');
            let cleaned = if is_negation { &trimmed[1..] } else { trimmed };
            let dir_only = cleaned.ends_with('/');
            let cleaned = if dir_only {
                cleaned.trim_end_matches('/')
            } else {
                cleaned
            };

            patterns.push(McIgnorePattern {
                pattern: cleaned.to_owned(),
                is_negation,
                dir_only,
            });
        }

        Ok(Self {
            patterns: Arc::new(patterns),
        })
    }

    /// Returns `true` if the given relative path should be ignored.
    ///
    /// Follows gitignore semantics: every pattern that matches the path is
    /// evaluated in order and the **last matching pattern wins**. A positive
    /// pattern marks the path ignored; a negation (`!`) re-includes it. This
    /// means a `!pattern` appearing after an earlier positive match re-includes
    /// the path.
    #[must_use]
    pub fn is_ignored(&self, relative_path: &str, is_dir: bool) -> bool {
        // Normalise — strip leading `./` and trailing `/` for matching.
        let normalised = relative_path.trim_start_matches("./").trim_end_matches('/');

        // Last-match-wins: keep applying every matching pattern; the final one
        // decides the outcome.
        let mut ignored = false;
        for p in self.patterns.iter() {
            if p.dir_only {
                let is_exact =
                    normalised == p.pattern || normalised.ends_with(&format!("/{}", p.pattern));
                if is_exact && !is_dir {
                    continue;
                }
            }
            if Self::matches(normalised, &p.pattern) {
                ignored = !p.is_negation;
            }
        }
        ignored
    }

    /// Simple glob matching. Supports `*` (single-segment), `**` (multi-segment).
    ///
    /// A non-glob pattern matches the path itself, a same-named entry at any
    /// level (`*/pattern`), or anything nested beneath it (`pattern/*`), so a
    /// directory pattern also ignores its contents (gitignore semantics).
    fn matches(path: &str, pattern: &str) -> bool {
        if !pattern.contains('*') {
            return path == pattern
                || path.ends_with(&format!("/{pattern}"))
                || path.starts_with(&format!("{pattern}/"));
        }

        // Convert the glob into a simple regex or do manual matching.
        // For Phase 2 we use a straightforward segment-by-segment approach.
        let path_segments: Vec<&str> = path.split('/').collect();
        let pat_segments: Vec<&str> = pattern.split('/').collect();

        Self::match_segments(&path_segments, &pat_segments, 0, 0)
    }

    fn match_segments(path_segs: &[&str], pat_segs: &[&str], pi: usize, pj: usize) -> bool {
        if pi == path_segs.len() && pj == pat_segs.len() {
            return true;
        }
        if pj == pat_segs.len() {
            return false;
        }

        if pat_segs[pj] == "**" {
            // ** matches zero or more segments
            for i in pi..=path_segs.len() {
                if Self::match_segments(path_segs, pat_segs, i, pj + 1) {
                    return true;
                }
            }
            return false;
        }

        if pi >= path_segs.len() {
            return false;
        }

        if pat_segs[pj] == "*" || pat_segs[pj] == path_segs[pi] {
            return Self::match_segments(path_segs, pat_segs, pi + 1, pj + 1);
        }
        if !pat_segs[pj].contains('*') {
            return false;
        }

        // Single-segment wildcard matching
        let pat = pat_segs[pj];
        let val = path_segs[pi];
        if Self::wildcard_match(val, pat) {
            return Self::match_segments(path_segs, pat_segs, pi + 1, pj + 1);
        }
        false
    }

    fn wildcard_match(value: &str, pattern: &str) -> bool {
        let v_chars: Vec<char> = value.chars().collect();
        let p_chars: Vec<char> = pattern.chars().collect();
        let mut vp = 0;
        let mut pp = 0;
        let mut star = None;

        loop {
            if pp < p_chars.len() && p_chars[pp] == '*' {
                star = Some((vp, pp));
                pp += 1;
                continue;
            }

            if vp < v_chars.len()
                && pp < p_chars.len()
                && (p_chars[pp] == '?' || p_chars[pp] == v_chars[vp])
            {
                vp += 1;
                pp += 1;
                continue;
            }

            if vp == v_chars.len() && pp == p_chars.len() {
                return true;
            }

            if let Some((sv, sp)) = star {
                if sv >= v_chars.len() {
                    return false;
                }
                vp = sv + 1;
                pp = sp + 1;
                star = Some((vp, sp));
                continue;
            }

            return false;
        }
    }
}

impl Default for McIgnore {
    /// An empty ignore set — nothing is excluded.
    fn default() -> Self {
        Self {
            patterns: Arc::new(Vec::new()),
        }
    }
}

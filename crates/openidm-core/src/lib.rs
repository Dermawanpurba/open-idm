//! OpenIDM core: download engine, queue, storage and browser protocol. No UI.

pub mod db;
pub mod naming;
pub mod settings;
pub mod protocol;
pub mod winutil;

#[cfg(test)]
mod tests {
    #[test]
    fn workspace_builds() {
        assert_eq!(2 + 2, 4);
    }
}

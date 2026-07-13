use tauri_plugin_window_state::StateFlags;

pub(crate) fn persisted_state_flags() -> StateFlags {
    StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED
}

#[cfg(test)]
mod tests {
    use super::persisted_state_flags;
    use tauri_plugin_window_state::StateFlags;

    #[test]
    fn persistence_policy_keeps_only_stable_window_geometry() {
        let flags = persisted_state_flags();

        for expected in [
            StateFlags::SIZE,
            StateFlags::POSITION,
            StateFlags::MAXIMIZED,
        ] {
            assert!(flags.contains(expected));
        }
        for excluded in [
            StateFlags::VISIBLE,
            StateFlags::DECORATIONS,
            StateFlags::FULLSCREEN,
        ] {
            assert!(!flags.contains(excluded));
        }
    }
}

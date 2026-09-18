#![cfg(test)]
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use super::to_wasapi_source;
use crate::desktop::{DesktopAudioSource, ProcessTreeScope};
use crate::windows_audio::{WasapiProcessTreeScope, WasapiSource};

fn process_loopback(tree_scope: ProcessTreeScope) -> WasapiSource {
    to_wasapi_source(&DesktopAudioSource::ProcessLoopback {
        process_id: 1234,
        tree_scope,
    })
}

/// Half of the facade → WASAPI scope translation. Pinned because the *other* half was
/// inverted for this enum's whole life (see `wasapi_process_tests.rs`), and a translation
/// layer is where such an inversion is cheapest to introduce and hardest to notice.
#[test]
fn include_children_translates_to_the_backend_include_scope() {
    assert_eq!(
        process_loopback(ProcessTreeScope::IncludeChildren),
        WasapiSource::ProcessLoopback {
            process_id: 1234,
            tree_scope: WasapiProcessTreeScope::IncludeChildren,
        }
    );
}

#[test]
fn exclude_process_tree_translates_to_the_backend_exclude_scope() {
    assert_eq!(
        process_loopback(ProcessTreeScope::ExcludeProcessTree),
        WasapiSource::ProcessLoopback {
            process_id: 1234,
            tree_scope: WasapiProcessTreeScope::ExcludeProcessTree,
        }
    );
}

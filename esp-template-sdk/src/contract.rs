//! Contract feature registry — the SDK version that introduced each fact, and
//! the minimum SDK version a template needs, computed from what it uses.
//!
//! ## One version, not two
//!
//! This crate's semver carries both meanings: the **major** is the spec version
//! (a breaking contract change bumps it), the **minor** the additive-feature
//! level. A template declares the version it was written against and
//! [`is_compatible`] is the whole gate.
//!
//! ## Scope: what the SDK implements, and nothing else
//!
//! Only names this crate provides *and enforces* belong here — those are the
//! only ones `sdk_version` can promise. Out of scope:
//!
//! * the template **language**, versioned by the engine's semver;
//! * **plugin surfaces** (`chip` and its fields, `plugin:` fragments), versioned
//!   by the plugin a template pins in `[plugins]` — see [`crate::plugin`];
//! * **host values** (`project_name`, …), which arrive through
//!   [`Facts::values`](crate::process::Facts::values) and which a host may
//!   supply none of.
//!
//! What is left is the predicates this crate registers, so everything here is
//! reserved: a template-scoped value may not shadow one.

use std::sync::LazyLock;

/// Contract versions are **strict semver**, ordering included: `2.0.0-rc.1 <
/// 2.0.0`, so an rc does not satisfy a requirement of `2.0.0`.
///
/// Host tool versions (rustc, probe-rs, …) are reported in looser formats and
/// want laxer prerelease handling. That is the host's concern, deliberately not
/// this type's.
pub use semver::Version;

/// The contract version this SDK implements: its own crate version.
pub static SDK_VERSION: LazyLock<Version> = LazyLock::new(|| {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version must be strict semver")
});

/// A final (non-prerelease) release version, constructible in `const` context —
/// `semver::Version::new` is not `const`.
pub const fn release(major: u64, minor: u64, patch: u64) -> Version {
    Version {
        major,
        minor,
        patch,
        pre: semver::Prerelease::EMPTY,
        build: semver::BuildMetadata::EMPTY,
    }
}

/// The fact API, grouped by the SDK release that introduced each batch.
///
/// **Adding a feature:** append it to the last group if that version is still
/// unreleased, otherwise start a new group at the current crate version.
const REGISTRY: &[(Version, &[&str])] = &[(
    // The whole fact API landed in the first SDK release.
    release(0, 1, 0),
    // Predicates, registered by `process::register_facts`.
    &["option", "group_selected"],
)];

/// One contract feature, as yielded by [`features()`].
#[derive(Clone, Debug, PartialEq, Eq)]
struct Feature {
    /// The name a template references.
    pub name: &'static str,
    /// The SDK release that introduced this feature.
    pub since: Version,
}

/// Every contract feature, oldest group first.
fn features() -> impl Iterator<Item = Feature> {
    REGISTRY.iter().flat_map(|(since, batch)| {
        batch.iter().map(move |name| Feature {
            name,
            since: since.clone(),
        })
    })
}

/// Look up a contract feature by the name a template references.
fn feature(name: &str) -> Option<Feature> {
    features().find(|f| f.name == name)
}

/// The lowest SDK version providing every one of `names`. An unrecognised name
/// contributes nothing — it belongs to a plugin or the host.
pub fn minimum_version<'a>(names: impl IntoIterator<Item = &'a str>) -> Version {
    let floor = REGISTRY
        .first()
        .map(|(since, _)| since.clone())
        .unwrap_or_else(|| release(0, 0, 0));

    names
        .into_iter()
        .filter_map(feature)
        .map(|f| f.since)
        .max()
        .unwrap_or(floor)
}

/// Whether `name` is one the SDK registers itself, so a template value must not
/// be registered over it. Derived from the registry rather than a second list.
pub(crate) fn is_reserved_name(name: &str) -> bool {
    feature(name).is_some()
}

/// Whether an SDK at `sdk` can run a template written against `required`:
/// same compatibility range, and `sdk >= required`.
///
/// Deliberately *not* `semver::VersionReq` caret matching, which excludes
/// prereleases outside the comparator's own version. `2.1.0-rc.1` satisfies
/// `2.0.0` here — esp-generate ships rcs, and locking their users out of every
/// template would be the wrong trade. `2.0.0-rc.1` still fails `2.0.0`.
pub fn is_compatible(sdk: &Version, required: &Version) -> bool {
    compatibility_range(sdk) == compatibility_range(required) && sdk >= required
}

/// The compatibility range a version belongs to, as Cargo's caret operator
/// defines it: `1.4.0` and `1.7.2` share one, `0.1.x` and `0.2.x` do not
/// (below 1.0 the minor carries breaking changes), and `0.0.x` releases are
/// each their own.
fn compatibility_range(v: &Version) -> (u64, u64, u64) {
    match (v.major, v.minor) {
        (0, 0) => (0, 0, v.patch),
        (0, minor) => (0, minor, 0),
        (major, _) => (major, 0, 0),
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn the_contract_version_is_the_crate_version() {
        assert_eq!(SDK_VERSION.to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn no_feature_claims_to_predate_the_sdk_that_ships_it() {
        // A group ahead of the crate version would make the SDK fail to
        // satisfy its own registry.
        for f in features() {
            assert!(
                is_compatible(&SDK_VERSION, &f.since),
                "feature `{}` since {} is not satisfied by this SDK ({})",
                f.name,
                f.since,
                *SDK_VERSION
            );
        }
    }

    #[test]
    fn registry_groups_are_ordered_and_distinct() {
        let versions: Vec<&Version> = REGISTRY.iter().map(|(v, _)| v).collect();
        for pair in versions.windows(2) {
            assert!(pair[0] < pair[1], "groups must be strictly increasing");
        }
    }

    #[test]
    fn every_registered_name_is_reserved_and_nothing_else_is() {
        // A name that fell out of the registry becomes silently shadowable; a
        // host value wrongly listed stops being registered at all.
        for f in features() {
            assert!(is_reserved_name(f.name), "`{}` must be reserved", f.name);
        }
        assert!(
            !is_reserved_name("project_name"),
            "a host value is supplied *through* `values` and must stay registerable"
        );
        assert!(!is_reserved_name("definitely_not_a_fact"));
    }

    #[test]
    fn host_supplied_values_are_not_registered() {
        for name in [
            "project_name",
            "generate_version",
            "generate_parameters",
            "rust_toolchain",
            "reserved_gpio_code",
            "has_reserved_pins",
        ] {
            assert!(
                feature(name).is_none(),
                "`{name}` is supplied by the host, so `sdk_version` cannot promise it"
            );
        }
    }

    #[test]
    fn the_purpose_picked_chip_facts_are_gone() {
        // Chip capabilities are `chip.<symbol>` fields now, so somni reports a
        // typo itself. These names must not come back as separate facts.
        for gone in ["chip_has", "is_xtensa", "is_riscv", "rust_target"] {
            assert!(
                feature(gone).is_none(),
                "`{gone}` is a plugin namespace field now, not an SDK feature"
            );
        }
    }

    #[test]
    fn contract_versions_are_strict_semver() {
        // Full three-component semver only: the contract is machine-authored
        // (a template declares it, `check` computes it), so there is no reason
        // to accept the abbreviations host tools get away with.
        assert_eq!(Version::parse("2.0.0").unwrap(), release(2, 0, 0));
        assert!(Version::parse("2.1").is_err());
        assert!(Version::parse("3").is_err());
        assert!(Version::parse("2.0.0.0").is_err());
        assert!(Version::parse("x").is_err());

        assert!(release(2, 0, 0) < release(2, 0, 1));
        assert!(release(2, 1, 0) > release(2, 0, 9));
        assert_eq!(release(2, 0, 0).to_string(), "2.0.0");
    }

    #[test]
    fn a_breaking_bump_is_not_compatible_in_either_direction() {
        // The major *is* the spec version, which is why the separate integer
        // was redundant.
        assert!(!is_compatible(&release(2, 0, 0), &release(1, 9, 9)));
        assert!(!is_compatible(&release(1, 9, 9), &release(2, 0, 0)));

        // Below 1.0 the minor carries breaking changes, so `0.1` and `0.2` are
        // as incompatible as `1.x` and `2.x`. This is the regime the SDK is in
        // today, so getting it wrong would be a live bug, not a future one.
        assert!(!is_compatible(&release(0, 2, 0), &release(0, 1, 0)));
        assert!(!is_compatible(&release(0, 1, 0), &release(0, 2, 0)));
        assert!(!is_compatible(&release(1, 2, 0), &release(0, 0, 0)));
        assert!(is_compatible(&release(0, 1, 5), &release(0, 1, 2)));

        // `0.0.x` releases are each their own range.
        assert!(!is_compatible(&release(0, 0, 2), &release(0, 0, 1)));
        assert!(is_compatible(&release(0, 0, 1), &release(0, 0, 1)));
    }

    #[test]
    fn a_newer_sdk_runs_an_older_template_but_not_the_reverse() {
        // "A template declaring x.y is compatible with x.y+n" — the review's
        // rule, which is the whole gate now.
        assert!(is_compatible(&release(1, 4, 0), &release(1, 2, 0)));
        assert!(is_compatible(&release(1, 2, 0), &release(1, 2, 0)));
        assert!(!is_compatible(&release(1, 2, 0), &release(1, 4, 0)));
    }

    #[test]
    fn a_prerelease_of_the_required_version_is_not_new_enough() {
        // The reason contract versions use semver rather than a stripped
        // `major.minor.patch`: an rc of 1.2.0 may not yet implement the feature
        // that a 1.2.0 requirement is gating.
        let rc = Version::parse("1.2.0-rc.1").unwrap();
        assert!(
            rc < release(1, 2, 0),
            "rc must sort below its final release"
        );
        assert!(!is_compatible(&rc, &release(1, 2, 0)));

        // An rc of a *later* release is fine — it contains everything 1.2.0
        // had. This is where we deliberately diverge from `VersionReq`, which
        // would reject it and lock rc users out of every template.
        assert!(is_compatible(
            &Version::parse("1.3.0-rc.1").unwrap(),
            &release(1, 2, 0)
        ));

        // Prereleases order among themselves.
        assert!(Version::parse("1.2.0-rc.1").unwrap() < Version::parse("1.2.0-rc.2").unwrap());
        assert!(Version::parse("1.2.0-alpha").unwrap() < Version::parse("1.2.0-beta").unwrap());

        // Build metadata: semver §10 excludes it from precedence, but the
        // crate *derives* `Ord`, so it does participate — `1.2.0+a` sorts above
        // `1.2.0`. Harmless here, because empty build metadata always sorts
        // first: a `+build` suffix can only ever help a version qualify, never
        // block it. Pinned so the deviation is recorded rather than latent.
        let with_build = Version::parse("1.2.0+a").unwrap();
        assert!(is_compatible(&with_build, &release(1, 2, 0)));
        assert!(with_build > release(1, 2, 0));
        assert_ne!(with_build, release(1, 2, 0));
    }

    #[test]
    fn every_feature_name_is_unique() {
        let mut seen = std::collections::HashSet::new();
        for f in features() {
            assert!(seen.insert(f.name), "duplicate feature name: {}", f.name);
        }
    }

    #[test]
    fn implemented_predicates_are_registered() {
        for name in ["option", "group_selected"] {
            assert!(
                feature(name).is_some(),
                "predicate `{name}` is not registered as a contract feature"
            );
        }
    }

    #[test]
    fn plugin_surfaces_are_not_sdk_features() {
        for name in ["chip", "chip.name", "chip.rust_target", "plugin:chip"] {
            assert!(
                feature(name).is_none(),
                "`{name}` comes from a plugin, so `sdk_version` cannot promise it"
            );
        }
    }

    #[test]
    fn the_language_is_not_this_registrys_business() {
        // Directive keywords are owned by the templating engine and versioned
        // by its own dependency semver, so they must not reappear here.
        for name in ["IF", "ELIF", "ELSE", "ENDIF", "REPLACE", "for", "include"] {
            assert!(
                feature(name).is_none(),
                "`{name}` is language surface — version it via the engine dependency"
            );
        }
    }
}

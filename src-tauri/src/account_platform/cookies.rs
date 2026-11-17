//! Cookie pairs added to one request by a Platform deployment.

use indexmap::IndexMap;

pub(super) fn pairs(header: &str) -> impl Iterator<Item = (&str, &str)> {
    header.split(';').filter_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        let name = name.trim();
        (!name.is_empty()).then(|| (name, value.trim()))
    })
}

fn header(pairs: impl IntoIterator<Item = (impl AsRef<str>, impl AsRef<str>)>) -> String {
    pairs
        .into_iter()
        .map(|(name, value)| format!("{}={}", name.as_ref(), value.as_ref()))
        .collect::<Vec<_>>()
        .join("; ")
}

pub(super) fn merge(base: &str, deployment: &str) -> String {
    let combined: IndexMap<&str, &str> = pairs(base).chain(pairs(deployment)).collect();
    header(combined)
}

#[cfg(any(windows, test))]
pub(super) fn strip_deployment(base: &str, deployment: &str) -> String {
    let deployed: IndexMap<&str, &str> = pairs(deployment).collect();
    header(pairs(base).filter(|(name, value)| deployed.get(name) != Some(value)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deployment_overrides_matching_names_and_keeps_other_cookies() {
        assert_eq!(
            merge(
                " session=abc; route=original; Route=separate ",
                "route=one; route=two==; empty=; malformed"
            ),
            "session=abc; route=two==; Route=separate; empty=",
        );
    }

    #[test]
    fn redirects_remove_deployed_values_and_keep_destination_cookies() {
        assert_eq!(
            strip_deployment(
                "destination=ok; route=private; Route=separate; route=native; empty=",
                "route=private;empty="
            ),
            "destination=ok; Route=separate; route=native",
        );
    }
}

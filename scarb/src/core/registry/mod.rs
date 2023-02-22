use anyhow::Result;
use async_trait::async_trait;

use crate::core::{ManifestDependency, Package, PackageId, Summary};

pub mod cache;
pub mod source_map;

#[async_trait(?Send)]
pub trait Registry {
    /// Attempt to find the packages that match a dependency request.
    async fn query(&mut self, dependency: &ManifestDependency) -> Result<Vec<Summary>>;

    /// Fetch full package by its ID.
    async fn download(&mut self, package_id: PackageId) -> Result<Package>;
}

#[cfg(test)]
pub(crate) mod mock {
    use std::collections::hash_map::Entry;
    use std::collections::{HashMap, HashSet};

    use anyhow::{anyhow, bail, Result};
    use async_trait::async_trait;
    use camino::Utf8PathBuf;
    use itertools::Itertools;

    use crate::core::package::PackageName;
    use crate::core::registry::Registry;
    use crate::core::{Manifest, ManifestDependency, Package, PackageId, SourceId, Summary};

    #[derive(Clone, Debug, Default)]
    pub struct MockRegistry {
        index: HashMap<(PackageName, SourceId), HashSet<PackageId>>,
        dependencies: HashMap<PackageId, Vec<ManifestDependency>>,
        packages: HashMap<PackageId, Package>,
    }

    impl MockRegistry {
        pub fn new() -> Self {
            let mut reg = Self::default();
            reg.put(
                PackageId::new(
                    PackageName::CORE,
                    crate::version::get().cairo.version.parse().unwrap(),
                    SourceId::for_std(),
                ),
                Vec::new(),
            );
            reg
        }

        pub fn put(&mut self, package_id: PackageId, mut dependencies: Vec<ManifestDependency>) {
            assert!(
                !self.has_package(package_id),
                "Package {package_id} is already in registry"
            );

            self.index
                .entry((package_id.name.clone(), package_id.source_id))
                .or_default()
                .insert(package_id);

            self.dependencies
                .entry(package_id)
                .or_default()
                .append(&mut dependencies);
        }

        pub fn has_package(&self, package_id: PackageId) -> bool {
            self.dependencies.contains_key(&package_id)
        }

        pub fn get_package(&mut self, package_id: PackageId) -> Result<Package> {
            if !self.has_package(package_id) {
                bail!("MockRegistry/get_package: unknown package {package_id}");
            }

            match self.packages.entry(package_id) {
                Entry::Occupied(entry) => Ok(entry.get().clone()),
                Entry::Vacant(entry) => {
                    let package =
                        Self::build_package(package_id, self.dependencies[&package_id].clone());
                    entry.insert(package.clone());
                    Ok(package)
                }
            }
        }

        fn build_package(package_id: PackageId, dependencies: Vec<ManifestDependency>) -> Package {
            let mut sb = Summary::build(package_id).with_dependencies(dependencies);

            if package_id.name == PackageName::CORE {
                sb = sb.no_core(true);
            }

            let summary = sb.finish();

            let manifest = Box::new(Manifest {
                summary,
                targets: Vec::new(),
                metadata: Default::default(),
            });

            Package::new(package_id, Utf8PathBuf::new(), manifest)
        }
    }

    #[async_trait(?Send)]
    impl Registry for MockRegistry {
        async fn query(&mut self, dependency: &ManifestDependency) -> Result<Vec<Summary>> {
            Ok(self
                .index
                .get(&(dependency.name.clone(), dependency.source_id))
                .ok_or_else(|| anyhow!("MockRegistry/query: cannot find {dependency}"))?
                .iter()
                .copied()
                .filter(|id| dependency.version_req.matches(&id.version))
                .sorted_unstable_by(|a, b| b.version.cmp(&a.version))
                .map(|id| self.get_package(id).unwrap().manifest.summary.clone())
                .collect())
        }

        async fn download(&mut self, package_id: PackageId) -> Result<Package> {
            self.get_package(package_id)
        }
    }

    macro_rules! registry {
        [$($x:tt),* $(,)?] => {{
            #[allow(unused_imports)]
            use $crate::core::registry::mock;
            #[allow(unused_mut)]
            let mut registry = mock::MockRegistry::new();
            $({
                let (package_id, dependencies) = mock::registry_entry!($x);
                registry.put(package_id, dependencies);
            })*
            registry
        }};
    }

    pub(crate) use registry;

    macro_rules! registry_entry {
        (($p:literal, [ $($d:tt),* $(,)? ] $(,)?)) => {{
            #[allow(unused_imports)]
            use $crate::core::registry::mock;
            let package_id = $crate::core::PackageId::from_display_str($p).unwrap();
            let dependencies = mock::deps![$($d),*].iter().cloned().collect();
            (package_id, dependencies)
        }};
    }

    pub(crate) use registry_entry;

    macro_rules! deps {
        [$($x:tt),* $(,)?] => (
            &[
                $($crate::core::registry::mock::dep!($x)),*
            ]
        );
    }

    pub(crate) use deps;

    macro_rules! dep {
        (($n:literal, $v:literal)) => {
            $crate::core::ManifestDependency {
                name: $crate::core::PackageName::new($n),
                version_req: ::semver::VersionReq::parse($v).unwrap(),
                source_id: $crate::core::SourceId::default_registry(),
            }
        };

        (($n:literal, $v:literal, $s:literal)) => {
            $crate::core::ManifestDependency {
                name: $crate::core::PackageName::new($n),
                version_req: ::semver::VersionReq::parse($v).unwrap(),
                source_id: $crate::core::SourceId::from_display_str($s).unwrap(),
            }
        };
    }

    pub(crate) use dep;

    macro_rules! pkgs {
        [$($x:expr),* $(,)?] => (
            &[
                $($crate::core::PackageId::from_display_str($x).unwrap()),*
            ]
        );
    }

    pub(crate) use pkgs;
}

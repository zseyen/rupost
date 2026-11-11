//! M2 验收 bench（06 DoD）：注册表缓存读取 ≤5ms（口径不含首轮目录扫描）。
use criterion::{Criterion, criterion_group, criterion_main};
use rupost::plugin::{PluginRegistry, RegistryCacheFile};
use std::path::PathBuf;

fn bench_registry_cache_read(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let cache_path = dir.path().join("registry.json");
    let mut manifests = Vec::new();
    for i in 0..16 {
        let bin = dir.path().join(format!("rupost-plugin-b{i}"));
        std::fs::write(&bin, "#!/bin/sh\nexit 0\n").unwrap();
        let manifest_path = dir.path().join(format!("rupost-plugin-b{i}.json"));
        std::fs::write(
            &manifest_path,
            format!(
                r#"{{"manifest_version":"1.0","name":"b{i}","version":"1","description":"d","commands":[{{"name":"b{i}","usage":"u","description":"d"}}],"hooks":[]}}"#
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let (mtime, hash) = rupost::plugin::registry::manifest_fingerprint(&manifest_path).unwrap();
        let manifest: rupost::plugin::PluginManifest =
            serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
        manifests.push(rupost::plugin::registry::CachedPluginEntry {
            executable_path: bin.to_string_lossy().to_string(),
            manifest_path: manifest_path.to_string_lossy().to_string(),
            mtime_secs: mtime,
            fingerprint_hash: hash,
            manifest,
        });
    }
    let cache = RegistryCacheFile {
        version: "1.0".to_string(),
        updated_at_utc: chrono::Utc::now().to_rfc3339(),
        manifests,
    };
    PluginRegistry::save_cache_atomic(&cache, &cache_path).unwrap();

    let mut group = c.benchmark_group("registry_cache_read");
    group.throughput(criterion::Throughput::Bytes(
        std::fs::metadata(&cache_path).unwrap().len(),
    ));
    group.bench_function("load_cache_16_plugins", |b| {
        b.iter(|| {
            let path = PathBuf::from(criterion::black_box(cache_path.to_str().unwrap()));
            assert!(PluginRegistry::load_cache(&path).is_some());
        })
    });
    group.finish();
}

criterion_group!(benches, bench_registry_cache_read);
criterion_main!(benches);

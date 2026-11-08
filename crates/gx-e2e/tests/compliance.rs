//! `TC-COMPLIANCE-01/02/03` 与 `TC-CBT-12`：`DEV-26` 资产与术语门禁。
//!
//! 白名单（经澄清 Q-10）：`docs/**`、`LEGAL/**`；其余工作树全量扫描。
//! 禁用词表来自 `docs/TERM_DENYLIST.txt`，测试代码本身不含任何词条字面量。
//! `TC-CBT-12` 校验表现层零外部资源（`GX-CBT-2026-001` §16.3）。

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root must exist")
}

fn denylist() -> Vec<String> {
    let path = workspace_root().join("docs/TERM_DENYLIST.txt");
    let raw = fs::read_to_string(&path).expect("denylist file must exist");
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

fn is_whitelisted(rel: &str) -> bool {
    const DIRS: [&str; 7] = [
        "docs/",
        "LEGAL/",
        "target/",
        ".git/",
        ".codebuddy/",
        ".idea/",
        ".vs/",
    ];
    DIRS.iter().any(|d| rel.starts_with(d)) || rel == "Cargo.lock"
}

fn is_scannable(path: &Path) -> bool {
    const EXTS: [&str; 14] = [
        "rs",
        "toml",
        "md",
        "yml",
        "yaml",
        "json",
        "txt",
        "ron",
        "ps1",
        "sh",
        "cfg",
        "ini",
        "lock",
        "gitignore",
    ];
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| EXTS.contains(&e))
        .unwrap_or(false)
        && fs::metadata(path)
            .map(|m| m.len() < 2_000_000)
            .unwrap_or(false)
}

fn is_ascii_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'\'' || b == b'.'
}

/// ASCII 词条按词边界匹配，中文词条按子串匹配。
fn contains_term(haystack: &str, term: &str) -> bool {
    if !term.is_ascii() {
        return haystack.contains(term);
    }
    let h = haystack.as_bytes();
    let n = needle_bytes(term);
    if n.is_empty() || n.len() > h.len() {
        return false;
    }
    (0..=h.len() - n.len()).any(|i| {
        if &h[i..i + n.len()] != n.as_slice() {
            return false;
        }
        let before_ok = i == 0 || !is_ascii_word_char(h[i - 1]);
        let after = i + n.len();
        let after_ok = after == h.len() || !is_ascii_word_char(h[after]);
        before_ok && after_ok
    })
}

fn needle_bytes(term: &str) -> Vec<u8> {
    term.as_bytes().to_vec()
}

fn collect_files(dir: &Path, root: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if is_whitelisted(&rel) {
            continue;
        }
        if path.is_dir() {
            collect_files(&path, root, out);
        } else if is_scannable(&path) {
            out.push((rel, path));
        }
    }
}

/// 递归收集发布树中的字体 / 图片 / 音频二进制（不受文本扫描扩展名限制）。
fn collect_bundled_assets(dir: &Path, root: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if is_whitelisted(&rel) {
            continue;
        }
        if path.is_dir() {
            collect_bundled_assets(&path, root, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| ["ttf", "ttc", "otf", "png", "jpg", "wav", "ogg"].contains(&e))
        {
            out.push(rel);
        }
    }
}

fn scan_violations() -> Vec<String> {
    let root = workspace_root();
    let terms = denylist();
    assert!(!terms.is_empty(), "denylist must not be empty");
    let mut files = Vec::new();
    collect_files(&root, &root, &mut files);
    let mut violations = Vec::new();
    for (rel, path) in files {
        for term in &terms {
            if contains_term(&rel, term) {
                violations.push(format!("filename: {rel} -> <term>"));
            }
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        for (line_no, line) in content.lines().enumerate() {
            for term in &terms {
                if contains_term(line, term) {
                    violations.push(format!("{rel}:{} -> <term>", line_no + 1));
                }
            }
        }
    }
    violations
}

#[test]
fn tc_compliance_01_no_denied_terms_in_shipped_tree() {
    let violations = scan_violations();
    assert!(
        violations.is_empty(),
        "TC-COMPLIANCE-01 failed: denied terms found in {} location(s):\n{}",
        violations.len(),
        violations.join("\n")
    );
}

#[test]
fn tc_compliance_02_asset_registry_complete() {
    let root = workspace_root();
    let registry =
        fs::read_to_string(root.join("ASSET_REGISTRY.md")).expect("ASSET_REGISTRY.md must exist");
    assert!(
        registry.contains("资产来源登记表"),
        "registry header missing"
    );
    assert!(registry.contains("许可"), "license column missing");
    for id in [
        "A-001", "A-002", "A-003", "A-004", "A-005", "A-006", "A-007",
    ] {
        assert!(registry.contains(id), "asset {id} not registered");
    }
    // 占位与外部依赖必须被显式标注，不得伪装成资产齐备
    assert!(
        registry.contains("待资产"),
        "placeholder assets must be flagged"
    );
    assert!(
        registry.contains("外部依赖"),
        "external dependencies must be flagged"
    );
    // 第三方资料必须隔离
    assert!(
        root.join("LEGAL/assets").is_dir(),
        "LEGAL/assets must exist"
    );
    assert!(
        root.join("LEGAL/dictionaries").is_dir(),
        "LEGAL/dictionaries must exist"
    );
    assert!(
        root.join("LEGAL/schemas").is_dir(),
        "LEGAL/schemas must exist"
    );
    assert!(
        registry.contains("不得进入发布包"),
        "release exclusion rule missing"
    );
}

/// `TC-CBT-12`（v1.2 修订）：**发布包**不含任何第三方表现资产；
/// 几何/纹理/图标全部代码生成；CJK 字形允许在运行时借用宿主 OS 已有字体，
/// 但必须有 ASCII 回退（`PRES-3 v1.2` / `GX-CBT-2026-001` §16.3）。
#[test]
fn tc_cbt_12_no_bundled_presentation_assets() {
    let root = workspace_root();

    // 1. 工作树根不得存在美术 / 字体 / 音频资源目录
    for dir in [
        "assets",
        "resources",
        "fonts",
        "audio",
        "models",
        "textures",
    ] {
        assert!(
            !root.join(dir).is_dir(),
            "external asset directory `{dir}` must not exist at workspace root"
        );
    }

    // 2. 发布树内不得出现任何字体 / 图片 / 音频二进制文件，也不得用 include_bytes! 内嵌。
    //    本文件是词表载体，跳过自身以免自匹配。
    let mut bundled = Vec::new();
    collect_bundled_assets(&root, &root, &mut bundled);
    let mut hits: Vec<String> = bundled
        .iter()
        .map(|r| format!("{r} -> bundled asset file"))
        .collect();

    let mut files = Vec::new();
    collect_files(&root, &root, &mut files);
    for (rel, path) in files {
        if rel.ends_with("tests/compliance.rs") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        if content.contains("include_bytes!") {
            hits.push(format!("{rel} -> include_bytes!"));
        }
    }
    assert!(
        hits.is_empty(),
        "TC-CBT-12: bundled/external resources found:\n{}",
        hits.join("\n")
    );

    // 3. 中文字形：允许运行时探测 OS 字体，但必须具备 ASCII 回退路径
    let ui_src = fs::read_to_string(root.join("crates/client/src/ui.rs")).expect("ui.rs missing");
    assert!(ui_src.contains("pub struct UiLanguage"));
    assert!(ui_src.contains("fn setup_cjk_font"));
    assert!(ui_src.contains("lang.cjk = false"));
    assert!(ui_src.contains("pub fn t("));

    // 4. 资产登记表中表现层条目的来源必须全部为「代码生成 / 引擎内置字形」
    let registry = fs::read_to_string(root.join("ASSET_REGISTRY.md")).expect("registry must exist");
    for id in ["A-001", "A-002", "A-003", "A-004", "A-005"] {
        let line = registry
            .lines()
            .find(|l| l.contains(id))
            .unwrap_or_else(|| panic!("asset {id} not registered"));
        assert!(
            line.contains("代码") || line.contains("引擎内置"),
            "{id} must be generated by code, got: {line}"
        );
        assert!(
            line.contains("已提供"),
            "{id} must be marked as provided in this delivery"
        );
    }

    // 4. 方块英雄由 ≥6 个体素拼装（PRES-6）。
    // 统计 `.spawn(` 调用而非字面 `b.spawn(`：P-V2 把腿/头改成「枢轴 + 子节点」后，
    // rustfmt 会把 `b.spawn(...).with_children(...)` 拆成 `b\n    .spawn(...)`，
    // 旧的字面计数会把等价体素误判为缺失。两种写法在此统一覆盖。
    let voxel_src =
        fs::read_to_string(root.join("crates/client/src/voxel.rs")).expect("voxel.rs must exist");
    let start = voxel_src
        .find("pub fn spawn_voxel_hero")
        .expect("spawn_voxel_hero must exist");
    let end = voxel_src[start..]
        .find("\n/// ")
        .map(|i| start + i)
        .unwrap_or(voxel_src.len());
    let body = &voxel_src[start..end];
    let spawn_count = body.matches(".spawn(").count();
    assert!(
        spawn_count >= 6,
        "voxel hero must be assembled from at least 6 cuboids, got {spawn_count} spawn calls"
    );
}

#[test]
fn tc_compliance_03_whitelist_is_bounded() {
    // 白名单只能是指定的两个目录 + 构建产物，不得扩大
    assert!(is_whitelisted("docs/anything.md"));
    assert!(is_whitelisted("LEGAL/assets/README.md"));
    assert!(!is_whitelisted("crates/client/src/ui.rs"));
    assert!(!is_whitelisted("README.md"));
    assert!(!is_whitelisted("ASSET_REGISTRY.md"));
}

use harbly_core::{Library, SortKey};
use std::fs;

fn html(title: &str, body: &str) -> String {
    format!("<!doctype html><html><head><title>{title}</title></head><body><h2>{body}</h2><script>var x=1;</script></body></html>")
}

/// Build a Markdown document, optionally with a YAML front-matter title.
fn md(front_title: Option<&str>, body: &str) -> String {
    match front_title {
        Some(t) => format!("---\ntitle: {t}\n---\n\n{body}"),
        None => body.to_string(),
    }
}

fn setup() -> (tempfile::TempDir, Library) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("Harbly");
    let lib = Library::open_or_create(&root).unwrap();
    (tmp, lib)
}

#[test]
fn scan_index_search_chinese() {
    let (_tmp, lib) = setup();
    let root = lib.root().to_path_buf();
    fs::create_dir_all(root.join("客户项目/星链SaaS")).unwrap();
    fs::write(
        root.join("客户项目/星链SaaS/季度营收仪表盘.html"),
        html("季度营收仪表盘", "Q2 营收总览，定价策略与净收入留存"),
    )
    .unwrap();
    fs::write(
        root.join("dashboard.html"),
        html("Revenue Dashboard", "quarterly pricing overview"),
    )
    .unwrap();

    let sum = lib.scan(|_| {}).unwrap();
    assert_eq!(sum.added, 2);
    assert_eq!(sum.total, 2);

    // Two-character Chinese word search (FTS5 can't do this natively; verifies the jieba path)
    let hits = lib.search("定价").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].asset.title, "季度营收仪表盘");

    // English prefix search
    let hits = lib.search("pric").unwrap();
    assert_eq!(hits.len(), 1);

    // Directory tree and counts
    let tree = lib.dir_tree().unwrap();
    assert_eq!(tree.count, 2);
    let proj = tree.children.iter().find(|c| c.name == "客户项目").unwrap();
    assert_eq!(proj.count, 1);
    assert_eq!(proj.children[0].name, "星链SaaS");

    // Listing: empty string = "All Assets", the whole library recursively; concrete folder = direct files only
    let all = lib.list_assets("", SortKey::Recent).unwrap();
    assert_eq!(all.len(), 2);
    let in_folder = lib
        .list_assets("客户项目/星链SaaS", SortKey::Recent)
        .unwrap();
    assert_eq!(in_folder.len(), 1);
    assert_eq!(in_folder[0].file_name, "季度营收仪表盘.html");
}

#[test]
fn import_dedup_and_rename() {
    let (tmp, lib) = setup();
    let outside = tmp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("a.html"), html("A", "内容甲")).unwrap();
    fs::write(outside.join("b.html"), html("B", "内容乙")).unwrap();
    fs::write(outside.join("readme.txt"), "not html").unwrap();

    let r = lib
        .import_files(
            &[
                outside.join("a.html"),
                outside.join("b.html"),
                outside.join("readme.txt"),
            ],
            "",
        )
        .unwrap();
    assert_eq!(r.added, 2);
    assert_eq!(r.skipped, 1);

    // Re-importing identical content → deduplicated
    let r2 = lib.import_files(&[outside.join("a.html")], "").unwrap();
    assert_eq!(r2.added, 0);
    assert_eq!(r2.duplicates, 1);

    // Same name, different content → auto-suffixed
    fs::write(outside.join("a.html"), html("A2", "内容甲改")).unwrap();
    let r3 = lib.import_files(&[outside.join("a.html")], "").unwrap();
    assert_eq!(r3.added, 1);
    assert_eq!(r3.renamed, 1);
    assert!(lib.root().join("a-2.html").exists());
}

#[test]
fn version_chain_external_edit_and_restore() {
    let (_tmp, lib) = setup();
    let f = lib.root().join("page.html");
    fs::write(&f, html("Page", "第一版内容")).unwrap();
    lib.scan(|_| {}).unwrap();

    let assets = lib.list_assets("", SortKey::Recent).unwrap();
    let id = assets[0].id.clone();
    assert_eq!(assets[0].ver_count, 1);

    // External edit (different length → size change triggers re-hashing)
    fs::write(&f, html("Page", "第二版内容，加长一些以确保 size 变化")).unwrap();
    let sum = lib.scan(|_| {}).unwrap();
    assert_eq!(sum.updated, 1);
    let vs = lib.list_versions(&id).unwrap();
    assert_eq!(vs.len(), 2);
    assert_eq!(vs[0].label, "外部修改");

    // Roll back to v1: content restored, and history gains an appended v3
    lib.restore_version(&id, 1).unwrap();
    let vs = lib.list_versions(&id).unwrap();
    assert_eq!(vs.len(), 3);
    let now = fs::read_to_string(&f).unwrap();
    assert!(now.contains("第一版内容"));

    // Version files are full snapshots
    let v2 = fs::read_to_string(lib.version_file_path(&id, 2)).unwrap();
    assert!(v2.contains("第二版内容"));
}

#[test]
fn folder_rename_and_duplicate() {
    let (_tmp, lib) = setup();
    let root = lib.root().to_path_buf();
    fs::create_dir_all(root.join("项目/子目录")).unwrap();
    fs::write(root.join("项目/首页.html"), html("首页", "甲")).unwrap();
    fs::write(root.join("项目/子目录/内页.html"), html("内页", "乙")).unwrap();
    lib.scan(|_| {}).unwrap();

    let before = lib.list_assets("项目", SortKey::Name).unwrap();
    let id = before[0].id.clone();

    // Rename a folder: paths rebound, asset ids unchanged, nothing deleted
    lib.rename_folder("项目", "项目X").unwrap();
    assert!(root.join("项目X/首页.html").exists());
    let after = lib.asset(&id).unwrap();
    assert_eq!(after.folder, "项目X");
    let tree = lib.dir_tree().unwrap();
    assert!(tree.children.iter().any(|c| c.name == "项目X"));

    // Duplicate a folder: recursive copy + new assets registered
    let dup = lib.duplicate_folder("项目X").unwrap();
    assert_eq!(dup, "项目X 副本");
    assert!(root.join("项目X 副本/子目录/内页.html").exists());
    assert_eq!(lib.total_count().unwrap(), 4);

    // Duplicate a file: same-directory copy with the "副本" suffix
    let copy = lib.duplicate_asset(&id).unwrap();
    assert!(copy.file_name.contains("副本"));
    assert_eq!(copy.folder, "项目X");
}

#[test]
fn tags_and_export() {
    let (tmp, lib) = setup();
    let root = lib.root().to_path_buf();
    fs::create_dir_all(root.join("目录")).unwrap();
    fs::write(root.join("目录/甲.html"), html("甲", "内容甲")).unwrap();
    fs::write(root.join("目录/乙.html"), html("乙", "内容乙")).unwrap();
    lib.scan(|_| {}).unwrap();

    let assets = lib.list_assets("目录", SortKey::Name).unwrap();
    assert!(assets[0].tags.is_empty());

    // Tag → tags come back in metadata → tag list → fetch by tag
    lib.set_tags(&assets[0].id, &["仪表盘".into(), "重要".into()])
        .unwrap();
    lib.set_tags(&assets[1].id, &["重要".into()]).unwrap();
    let a = lib.asset(&assets[0].id).unwrap();
    assert_eq!(a.tags.len(), 2);
    let tags = lib.all_tags().unwrap();
    assert_eq!(tags[0].name, "重要");
    assert_eq!(tags[0].count, 2);
    assert_eq!(lib.assets_by_tag("仪表盘").unwrap().len(), 1);

    // Search hits on a tag word
    let hits = lib.search("仪表盘").unwrap();
    assert!(hits.iter().any(|h| h.asset.id == assets[0].id));

    // Export zip
    let dest = tmp.path().join("out.zip");
    let n = lib.export_folder_zip("目录", &dest).unwrap();
    assert_eq!(n, 2);
    assert!(dest.metadata().unwrap().len() > 0);
}

#[test]
fn move_rename_inbox() {
    let (_tmp, lib) = setup();
    fs::write(
        lib.root().join("_inbox").join("捕获.html"),
        html("新捕获", "插件收藏内容"),
    )
    .unwrap();
    fs::write(lib.root().join("x.html"), html("X", "正文")).unwrap();
    lib.scan(|_| {}).unwrap();
    assert_eq!(lib.inbox_count().unwrap(), 1);

    // Archiving = moving out of the inbox
    let inbox = lib.list_assets("_inbox", SortKey::Recent).unwrap();
    lib.create_folder("", "项目A").unwrap();
    let moved = lib.move_asset(&inbox[0].id, "项目A").unwrap();
    assert_eq!(moved.folder, "项目A");
    assert!(lib.root().join("项目A/捕获.html").exists());
    assert_eq!(lib.inbox_count().unwrap(), 0);

    // Rename keeps the extension
    let all = lib.list_assets("", SortKey::Recent).unwrap();
    let x = all.iter().find(|a| a.file_name == "x.html").unwrap();
    let renamed = lib.rename_asset(&x.id, "改名").unwrap();
    assert_eq!(renamed.file_name, "改名.html");
    assert!(lib.root().join("改名.html").exists());

    // Finder move (simulated): raw rename then rescan; rebound by hash without losing versions
    fs::rename(
        lib.root().join("改名.html"),
        lib.root().join("项目A/改名.html"),
    )
    .unwrap();
    let sum = lib.scan(|_| {}).unwrap();
    assert_eq!(sum.moved, 1);
    assert_eq!(sum.removed, 0);
    let a = lib.list_assets("项目A", SortKey::Name).unwrap();
    assert_eq!(a.len(), 2);
}

#[test]
fn md_scan_title_precedence_and_chinese_fts() {
    let (_tmp, lib) = setup();
    let root = lib.root().to_path_buf();
    // Front-matter title wins over the H1
    fs::write(
        root.join("frontmatter.md"),
        md(Some("我的笔记"), "# 别的标题\n\n正文包含 定价 策略与净收入"),
    )
    .unwrap();
    // No front matter → first H1 is the title
    fs::write(
        root.join("heading.md"),
        md(None, "# 季度报告\n\n一些中文内容"),
    )
    .unwrap();
    // Neither → falls back to the file stem
    fs::write(
        root.join("plain.md"),
        md(None, "just plain text, no heading"),
    )
    .unwrap();

    let sum = lib.scan(|_| {}).unwrap();
    assert_eq!(sum.added, 3);

    let all = lib.list_assets("", SortKey::Name).unwrap();
    let title_of = |name: &str| {
        all.iter()
            .find(|a| a.file_name == name)
            .unwrap()
            .title
            .clone()
    };
    assert_eq!(title_of("frontmatter.md"), "我的笔记");
    assert_eq!(title_of("heading.md"), "季度报告");
    assert_eq!(title_of("plain.md"), "plain"); // file stem

    // Two-character Chinese word search hits the Markdown body (jieba path)
    let hits = lib.search("定价").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].asset.file_name, "frontmatter.md");
}

#[test]
fn md_rename_preserves_extension() {
    let (_tmp, lib) = setup();
    fs::write(lib.root().join("note.md"), md(None, "# 甲\n\n内容")).unwrap();
    fs::write(lib.root().join("page.html"), html("Page", "content")).unwrap();
    lib.scan(|_| {}).unwrap();

    let all = lib.list_assets("", SortKey::Name).unwrap();
    let note = all.iter().find(|a| a.file_name == "note.md").unwrap();
    let page = all.iter().find(|a| a.file_name == "page.html").unwrap();

    // Markdown keeps .md even without a typed extension
    let r = lib.rename_asset(&note.id, "改名").unwrap();
    assert_eq!(r.file_name, "改名.md");
    // Typing the wrong managed extension does not convert the type
    let r = lib.rename_asset(&note.id, "again.html").unwrap();
    assert_eq!(r.file_name, "again.md");
    // HTML still behaves as before
    let r = lib.rename_asset(&page.id, "renamed").unwrap();
    assert_eq!(r.file_name, "renamed.html");

    // An extension-only rename is rejected (would create a hidden ".md" file)
    assert!(lib.rename_asset(&note.id, ".md").is_err());
    assert!(lib.rename_asset(&note.id, ".html").is_err());
    assert!(lib.root().join("again.md").exists()); // unchanged from the earlier rename
}

#[test]
fn md_version_chain_uses_md_extension() {
    let (_tmp, lib) = setup();
    let f = lib.root().join("doc.md");
    fs::write(&f, md(None, "# 标题\n\n第一版")).unwrap();
    lib.scan(|_| {}).unwrap();
    let id = lib.list_assets("", SortKey::Recent).unwrap()[0].id.clone();

    fs::write(
        &f,
        md(None, "# 标题\n\n第二版内容，加长一些以确保 size 变化"),
    )
    .unwrap();
    let sum = lib.scan(|_| {}).unwrap();
    assert_eq!(sum.updated, 1);

    // Version snapshots carry the asset's own extension
    assert!(lib.version_file_path(&id, 1).exists());
    assert!(lib.version_file_path(&id, 2).ends_with("v2.md"));
    let v2 = fs::read_to_string(lib.version_file_path(&id, 2)).unwrap();
    assert!(v2.contains("第二版内容"));

    // Rollback restores content and appends a new version
    lib.restore_version(&id, 1).unwrap();
    assert!(fs::read_to_string(&f).unwrap().contains("第一版"));
    assert_eq!(lib.list_versions(&id).unwrap().len(), 3);
}

#[test]
fn write_text_autosaves_without_versioning() {
    let (_tmp, lib) = setup();
    let a = lib.create_markdown_asset("", "笔记").unwrap();
    let base_hash = a.current_hash.clone();
    assert_eq!(a.file_name, "笔记.md");
    assert_eq!(a.ver_count, 1);

    // Two autosaves: content + title update, but no new version rows
    lib.write_asset_text(&a.id, "# 初稿\n\n一些内容").unwrap();
    let a2 = lib
        .write_asset_text(&a.id, "# 定稿\n\n更完整的内容")
        .unwrap();
    assert_eq!(a2.ver_count, 1);
    assert_eq!(a2.title, "定稿");

    // A scan sees the in-app write as already-indexed → no diff, no new version
    let sum = lib.scan(|_| {}).unwrap();
    assert!(!sum.changed());
    assert_eq!(lib.asset(&a.id).unwrap().ver_count, 1);

    // Ending the session checkpoints exactly one "编辑" version
    let ver = lib.checkpoint_version(&a.id, &base_hash).unwrap();
    assert_eq!(ver, Some(2));
    let vs = lib.list_versions(&a.id).unwrap();
    assert_eq!(vs.len(), 2);
    assert_eq!(vs[0].label, "编辑");

    // Checkpointing again with the same base is idempotent (dedup guard)
    assert_eq!(lib.checkpoint_version(&a.id, &base_hash).unwrap(), Some(2));
    assert_eq!(lib.list_versions(&a.id).unwrap().len(), 2);
}

#[test]
fn import_mixed_html_and_markdown() {
    let (tmp, lib) = setup();
    let outside = tmp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("a.html"), html("A", "内容甲")).unwrap();
    fs::write(outside.join("b.md"), md(None, "# B\n\n内容乙")).unwrap();
    fs::write(outside.join("c.txt"), "plain text").unwrap();

    let r = lib
        .import_files(
            &[
                outside.join("a.html"),
                outside.join("b.md"),
                outside.join("c.txt"),
            ],
            "",
        )
        .unwrap();
    assert_eq!(r.added, 2);
    assert_eq!(r.skipped, 1);
    assert!(harbly_core::is_managed_name("b.md"));
    assert!(!harbly_core::is_managed_name("c.txt"));
}

#[test]
fn md_render_rewrites_relative_images() {
    // Relative images are rewritten to resolve through the asset protocol
    let out = harbly_core::md_to_html_body(
        "![alt](img/a.png)",
        Some("harbly-asset://localhost/rel/ID/"),
    );
    assert!(out.contains("harbly-asset://localhost/rel/ID/img/a.png"));
    // Absolute URLs are left untouched
    let out = harbly_core::md_to_html_body("![alt](https://example.com/a.png)", Some("base/"));
    assert!(out.contains("https://example.com/a.png"));
    // A space in the path (angle-bracket destination) is percent-encoded
    let out = harbly_core::md_to_html_body("![](<my pic.png>)", Some("b/"));
    assert!(out.contains("b/my%20pic.png"));
}

/// Finder tag interop: set_tags writes the xattr; Finder-side edits (color-number format) are adopted by scans; duplicates inherit tags
#[cfg(target_os = "macos")]
#[test]
fn finder_tags_xattr_interop() {
    let (_tmp, lib) = setup();
    let abs = lib.root().join("笔记.html");
    fs::write(&abs, html("笔记", "灵感内容")).unwrap();
    lib.scan(|_| {}).unwrap();
    let id = lib.list_assets("", SortKey::Name).unwrap()[0].id.clone();

    // Set in-app → lands in the file xattr (visible to Finder)
    lib.set_tags(&id, &["重要".into(), "灵感".into()]).unwrap();
    assert_eq!(
        harbly_core::read_file_tags(&abs),
        vec!["重要".to_string(), "灵感".to_string()]
    );

    // Simulate a Finder edit: write entries carrying a color number ("name\n6") → after a scan the database follows disk
    harbly_core::write_file_tags(&abs, &["红色标签\n6".into(), "工作".into()]).unwrap();
    let sum = lib.scan(|_| {}).unwrap();
    assert_eq!(sum.tags_synced, 1);
    assert!(sum.changed());
    let mut tags = lib.asset(&id).unwrap().tags;
    tags.sort();
    assert_eq!(tags, vec!["工作".to_string(), "红色标签".to_string()]);

    // A second scan with no diff → not counted as a change (watcher echo suppression relies on this)
    let sum2 = lib.scan(|_| {}).unwrap();
    assert_eq!(sum2.tags_synced, 0);
    assert!(!sum2.changed());

    // The duplicate inherits tags (same behavior as Finder), with the color suffix preserved verbatim
    let dup = lib.duplicate_asset(&id).unwrap();
    let mut dup_tags = dup.tags.clone();
    dup_tags.sort();
    assert_eq!(dup_tags, vec!["工作".to_string(), "红色标签".to_string()]);

    // Clearing = removing the attribute
    lib.set_tags(&id, &[]).unwrap();
    assert!(harbly_core::read_file_tags(&abs).is_empty());
}

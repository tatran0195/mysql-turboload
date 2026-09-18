#![cfg(unix)]

use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn test_end_to_end_parallel_import() {
    let temp = tempdir().unwrap();
    let temp_path = temp.path();

    let backup_dir = temp_path.join("dumps");
    let log_dir = temp_path.join("logs");
    fs::create_dir_all(&backup_dir).unwrap();
    fs::create_dir_all(&log_dir).unwrap();

    // 1. Create SQL dump files
    let users_sql = backup_dir.join("users.sql");
    let mut f1 = File::create(&users_sql).unwrap();
    writeln!(f1, "-- Current Database: `shop_db`").unwrap();
    writeln!(f1, "CREATE TABLE users (id INT PRIMARY KEY);").unwrap();
    writeln!(f1, "INSERT INTO users VALUES (1), (2);").unwrap();

    let orders_sql = backup_dir.join("orders.sql");
    let mut f2 = File::create(&orders_sql).unwrap();
    writeln!(f2, "-- Host: localhost    Database: shop_db").unwrap();
    writeln!(f2, "CREATE TABLE orders (id INT PRIMARY KEY);").unwrap();

    let analytics_sql = backup_dir.join("analytics.sql");
    let mut f3 = File::create(&analytics_sql).unwrap();
    writeln!(f3, "-- Database: `stats_db`").unwrap();
    writeln!(f3, "CREATE TABLE page_views (view_id INT);").unwrap();

    let bad_sql = backup_dir.join("broken.sql");
    let mut f4 = File::create(&bad_sql).unwrap();
    writeln!(f4, "-- Database: `shop_db`").unwrap();
    writeln!(f4, "INVALID SQL SYNTAX FAIL HERE;").unwrap();

    // 2. Create mock mysql executable
    let mock_mysql = temp_path.join("mock_mysql.sh");
    let script = r#"#!/usr/bin/env bash
# Mock mysql executable for testing
for arg in "$@"; do
    if [[ "$arg" == "-e" ]]; then
        # Schema verification query
        exit 0
    fi
done

# Read stdin
content=$(cat)
if [[ "$content" == *"INVALID SQL SYNTAX FAIL HERE"* ]]; then
    echo "ERROR 1064 (42000) at line 2: You have an error in your SQL syntax" >&2
    exit 1
fi

exit 0
"#;
    fs::write(&mock_mysql, script).unwrap();
    let mut perms = fs::metadata(&mock_mysql).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&mock_mysql, perms).unwrap();

    // 3. Locate compiled binary
    let bin_path = env!("CARGO_BIN_EXE_mysql-turboload");

    // 4. Run first import
    let output = Command::new(bin_path)
        .arg("--dir")
        .arg(&backup_dir)
        .arg("--log-dir")
        .arg(&log_dir)
        .arg("--mysql-bin")
        .arg(&mock_mysql)
        .arg("--password")
        .arg("testpwd")
        .arg("--workers")
        .arg("2")
        .arg("--no-progress")
        .output()
        .expect("Failed to execute binary");

    // Exit code should be 1 because broken.sql failed
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    println!("STDOUT:\n{}", stdout);
    println!("STDERR:\n{}", stderr);

    // Verify manifest has completed the 3 good files
    let manifest_path = log_dir.join("manifest.json");
    assert!(manifest_path.is_file(), "Manifest should be created");
    let manifest_content = fs::read_to_string(&manifest_path).unwrap();
    assert!(manifest_content.contains("users.sql"));
    assert!(manifest_content.contains("orders.sql"));
    assert!(manifest_content.contains("analytics.sql"));
    assert!(!manifest_content.contains("broken.sql"));

    // Verify error log for broken.sql was created
    let err_log = log_dir.join("shop_db__broken.sql.err.log");
    assert!(err_log.is_file(), "Error log should be created");
    let err_content = fs::read_to_string(&err_log).unwrap();
    assert!(err_content.contains("ERROR 1064"));

    // Verify failed_files.txt was generated
    let failed_files_path = log_dir.join("failed_files.txt");
    assert!(failed_files_path.is_file());
    let failed_files_content = fs::read_to_string(&failed_files_path).unwrap();
    assert!(failed_files_content.contains("broken.sql"));

    // 5. Test --resume: re-running with --resume should skip the 3 completed files
    let resume_output = Command::new(bin_path)
        .arg("--dir")
        .arg(&backup_dir)
        .arg("--log-dir")
        .arg(&log_dir)
        .arg("--mysql-bin")
        .arg(&mock_mysql)
        .arg("--password")
        .arg("testpwd")
        .arg("--resume")
        .arg("--no-progress")
        .output()
        .expect("Failed to execute binary");

    let resume_stdout = String::from_utf8_lossy(&resume_output.stdout);
    assert!(resume_stdout.contains("Resuming: skipped 3 previously completed tables."));

    // 6. Test --retry-file: only imports files from the list
    let retry_output = Command::new(bin_path)
        .arg("--retry-file")
        .arg(&failed_files_path)
        .arg("--log-dir")
        .arg(&log_dir)
        .arg("--mysql-bin")
        .arg(&mock_mysql)
        .arg("--password")
        .arg("testpwd")
        .arg("--no-progress")
        .output()
        .expect("Failed to execute binary");

    let retry_stdout = String::from_utf8_lossy(&retry_output.stdout);
    assert!(retry_stdout.contains("SQL Files     : 1"));

    // 7. Test --dry-run
    let dry_run_output = Command::new(bin_path)
        .arg("--dir")
        .arg(&backup_dir)
        .arg("--log-dir")
        .arg(&log_dir)
        .arg("--mysql-bin")
        .arg(&mock_mysql)
        .arg("--dry-run")
        .output()
        .expect("Failed to execute binary");

    assert!(dry_run_output.status.success());
    let dry_stdout = String::from_utf8_lossy(&dry_run_output.stdout);
    assert!(dry_stdout.contains("[DRY RUN] Plan displayed."));
    assert!(dry_stdout.contains("shop_db"));
    assert!(dry_stdout.contains("stats_db"));
}

#[test]
fn test_end_to_end_parallel_export_and_roundtrip() {
    let temp = tempdir().unwrap();
    let temp_path = temp.path();

    let export_dir = temp_path.join("exported_dumps");
    let mock_mysql = temp_path.join("mock_mysql_for_export.sh");
    let mock_mysqldump = temp_path.join("mock_mysqldump.sh");

    // 1. Mock mysql for schema query
    let mysql_script = r#"#!/usr/bin/env bash
full_args="$*"
if [[ "$full_args" == *"information_schema.TABLES"* ]]; then
    printf "crm_db\tcustomers\t5242880\t10000\n"
    printf "crm_db\tleads\t1048576\t2000\n"
    printf "crm_db\tinvoices\t8388608\t15000\n"
    exit 0
fi
exit 0
"#;
    fs::write(&mock_mysql, mysql_script).unwrap();
    let mut perms1 = fs::metadata(&mock_mysql).unwrap().permissions();
    perms1.set_mode(0o755);
    fs::set_permissions(&mock_mysql, perms1).unwrap();

    // 2. Mock mysqldump for table data extraction
    let mysqldump_script = r#"#!/usr/bin/env bash
table="${@: -1}"
echo "DROP TABLE IF EXISTS \`$table\`;"
echo "CREATE TABLE \`$table\` (id INT PRIMARY KEY);"
echo "INSERT INTO \`$table\` VALUES (1), (2);"
exit 0
"#;
    fs::write(&mock_mysqldump, mysqldump_script).unwrap();
    let mut perms2 = fs::metadata(&mock_mysqldump).unwrap().permissions();
    perms2.set_mode(0o755);
    fs::set_permissions(&mock_mysqldump, perms2).unwrap();

    let bin_path = env!("CARGO_BIN_EXE_mysql-turboload");

    // 3. Run parallel export
    let export_output = Command::new(bin_path)
        .arg("export")
        .arg("--database")
        .arg("crm_db")
        .arg("--output-dir")
        .arg(&export_dir)
        .arg("--mysql-bin")
        .arg(&mock_mysql)
        .arg("--mysqldump-bin")
        .arg(&mock_mysqldump)
        .arg("--password")
        .arg("testpwd")
        .arg("--workers")
        .arg("2")
        .arg("--no-progress")
        .output()
        .expect("Failed to execute export");

    let stdout = String::from_utf8_lossy(&export_output.stdout);
    let stderr = String::from_utf8_lossy(&export_output.stderr);
    println!("EXPORT STDOUT:\n{}", stdout);
    println!("EXPORT STDERR:\n{}", stderr);
    assert!(export_output.status.success());

    // 4. Verify exported files exist and have standard headers
    let invoices_file = export_dir.join("invoices.sql");
    let customers_file = export_dir.join("customers.sql");
    let leads_file = export_dir.join("leads.sql");

    assert!(invoices_file.is_file());
    assert!(customers_file.is_file());
    assert!(leads_file.is_file());

    let manifest_file = export_dir.join("logs").join("manifest.json");
    assert!(
        manifest_file.is_file(),
        "Export manifest should be generated in export_dir/logs"
    );
    assert!(
        !export_dir.join("import-logs").exists(),
        "import-logs directory should not exist"
    );

    let invoice_content = fs::read_to_string(&invoices_file).unwrap();
    assert!(invoice_content.contains("-- Current Database: `crm_db`"));
    assert!(invoice_content.contains("CREATE TABLE `invoices`"));

    // 5. Test Roundtrip: Point mysql-turboload import at the exported folder!
    let import_dry_run = Command::new(bin_path)
        .arg("import")
        .arg("--dir")
        .arg(&export_dir)
        .arg("--mysql-bin")
        .arg(&mock_mysql)
        .arg("--dry-run")
        .output()
        .expect("Failed to run import on exported files");

    assert!(import_dry_run.status.success());
    let import_stdout = String::from_utf8_lossy(&import_dry_run.stdout);
    println!("IMPORT ROUNDTRIP STDOUT:\n{}", import_stdout);
    assert!(import_stdout.contains("crm_db"));
    assert!(import_stdout.contains("SQL Files     : 3"));
}

#[test]
fn test_end_to_end_parallel_export_compressed_and_roundtrip() {
    let temp = tempdir().unwrap();
    let temp_path = temp.path();

    // Nested non-existent directory to test auto-creation
    let export_dir = temp_path.join("nested_folder").join("compressed_dumps");
    let mock_mysql = temp_path.join("mock_mysql_for_export.sh");
    let mock_mysqldump = temp_path.join("mock_mysqldump.sh");

    // 1. Mock mysql for schema query and import execution
    let mysql_script = r#"#!/usr/bin/env bash
full_args="$*"
if [[ "$full_args" == *"information_schema.TABLES"* ]]; then
    printf "crm_db\tcustomers\t5242880\t10000\n"
    printf "crm_db\tinvoices\t8388608\t15000\n"
    exit 0
fi
# Decompressed stream test
content=$(cat)
if [[ "$content" == *"CREATE TABLE \`invoices\`"* ]]; then
    exit 0
fi
exit 0
"#;
    fs::write(&mock_mysql, mysql_script).unwrap();
    let mut perms1 = fs::metadata(&mock_mysql).unwrap().permissions();
    perms1.set_mode(0o755);
    fs::set_permissions(&mock_mysql, perms1).unwrap();

    // 2. Mock mysqldump
    let mysqldump_script = r#"#!/usr/bin/env bash
table="${@: -1}"
echo "DROP TABLE IF EXISTS \`$table\`;"
echo "CREATE TABLE \`$table\` (id INT PRIMARY KEY);"
echo "INSERT INTO \`$table\` VALUES (1), (2);"
exit 0
"#;
    fs::write(&mock_mysqldump, mysqldump_script).unwrap();
    let mut perms2 = fs::metadata(&mock_mysqldump).unwrap().permissions();
    perms2.set_mode(0o755);
    fs::set_permissions(&mock_mysqldump, perms2).unwrap();

    let bin_path = env!("CARGO_BIN_EXE_mysql-turboload");

    // 3. Run parallel export with --dir alias and --compress
    let export_output = Command::new(bin_path)
        .arg("export")
        .arg("--database")
        .arg("crm_db")
        .arg("--dir") // testing --dir alias
        .arg(&export_dir)
        .arg("--compress")
        .arg("--mysql-bin")
        .arg(&mock_mysql)
        .arg("--mysqldump-bin")
        .arg(&mock_mysqldump)
        .arg("--password")
        .arg("testpwd")
        .arg("--workers")
        .arg("2")
        .arg("--no-progress")
        .output()
        .expect("Failed to execute export");

    let stdout = String::from_utf8_lossy(&export_output.stdout);
    let stderr = String::from_utf8_lossy(&export_output.stderr);
    println!("EXPORT COMPRESSED STDOUT:\n{}", stdout);
    println!("EXPORT COMPRESSED STDERR:\n{}", stderr);
    assert!(export_output.status.success());

    // 4. Verify directory was created and .sql.gz files exist
    assert!(
        export_dir.is_dir(),
        "Output directory should be auto-created"
    );
    let invoices_file = export_dir.join("invoices.sql.gz");
    let customers_file = export_dir.join("customers.sql.gz");

    assert!(invoices_file.is_file(), "invoices.sql.gz should exist");
    assert!(customers_file.is_file(), "customers.sql.gz should exist");

    // Verify invoices.sql.gz is valid Gzip containing expected decompressed content
    let f = File::open(&invoices_file).unwrap();
    let mut decoder = flate2::read::GzDecoder::new(f);
    let mut decompressed = String::new();
    std::io::Read::read_to_string(&mut decoder, &mut decompressed).unwrap();
    assert!(decompressed.contains("-- Current Database: `crm_db`"));
    assert!(decompressed.contains("CREATE TABLE `invoices`"));

    // 5. Verify roundtrip: import directly reads .sql.gz files
    let import_output = Command::new(bin_path)
        .arg("import")
        .arg("--dir")
        .arg(&export_dir)
        .arg("--mysql-bin")
        .arg(&mock_mysql)
        .arg("--password")
        .arg("testpwd")
        .arg("--no-progress")
        .output()
        .expect("Failed to run import on exported files");

    assert!(import_output.status.success());
    let import_stdout = String::from_utf8_lossy(&import_output.stdout);
    println!("IMPORT ROUNDTRIP STDOUT:\n{}", import_stdout);
    assert!(import_stdout.contains("crm_db"));
    assert!(import_stdout.contains("SQL Files     : 2"));
}

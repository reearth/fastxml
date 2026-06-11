//! Test data downloader for conformance test suites.

use std::fs::{self, File};
use std::io::{self, BufReader};
use std::path::Path;

use flate2::read::GzDecoder;

/// Test suite metadata for downloading.
#[derive(Debug, Clone)]
pub struct TestSuiteSource {
    /// Name of the test suite (used as directory name).
    pub name: &'static str,
    /// URL to download the test data.
    pub url: &'static str,
    /// Type of archive.
    pub archive_type: ArchiveType,
    /// Description of the test suite.
    pub description: &'static str,
}

/// Type of archive for test data.
#[derive(Debug, Clone, Copy)]
pub enum ArchiveType {
    /// tar.gz archive
    TarGz,
    /// Git repository (requires git clone)
    Git,
}

/// Available test suites.
pub static TEST_SUITES: &[TestSuiteSource] = &[
    TestSuiteSource {
        name: "w3c-xml",
        url: "https://www.w3.org/XML/Test/xmlts20130923.tar.gz",
        archive_type: ArchiveType::TarGz,
        description: "W3C XML Conformance Test Suite (2013 edition)",
    },
    TestSuiteSource {
        name: "w3c-xsd",
        url: "https://github.com/w3c/xsdtests.git",
        archive_type: ArchiveType::Git,
        description: "W3C XML Schema Test Suite",
    },
];

/// Download and extract a test suite.
pub fn download_test_suite(suite: &TestSuiteSource, dest_dir: &Path) -> io::Result<()> {
    let suite_dir = dest_dir.join(suite.name);

    // Create destination directory
    fs::create_dir_all(&suite_dir)?;

    match suite.archive_type {
        ArchiveType::TarGz => download_and_extract_tar_gz(suite.url, &suite_dir)?,
        ArchiveType::Git => clone_git_repo(suite.url, &suite_dir)?,
    }

    Ok(())
}

/// Download and extract a tar.gz archive.
fn download_and_extract_tar_gz(url: &str, dest_dir: &Path) -> io::Result<()> {
    eprintln!("Downloading: {}", url);

    // Download the file
    let response = ureq::get(url)
        .call()
        .map_err(|e| io::Error::other(format!("Failed to download {}: {}", url, e)))?;

    // Create a temporary file for the download
    let temp_file = tempfile::NamedTempFile::new()?;
    let mut temp_writer = File::create(temp_file.path())?;

    // Copy response body to temp file
    let mut reader = response.into_reader();
    io::copy(&mut reader, &mut temp_writer)?;

    eprintln!("Extracting to: {}", dest_dir.display());

    // Open the temp file for reading
    let tar_gz_file = File::open(temp_file.path())?;
    let buf_reader = BufReader::new(tar_gz_file);
    let decoder = GzDecoder::new(buf_reader);
    let mut archive = tar::Archive::new(decoder);

    // Extract all files
    archive.unpack(dest_dir)?;

    eprintln!("Done!");
    Ok(())
}

/// Clone a git repository.
fn clone_git_repo(url: &str, dest_dir: &Path) -> io::Result<()> {
    eprintln!("Cloning: {}", url);

    // Use git command for cloning
    let status = std::process::Command::new("git")
        .args(["clone", "--depth", "1", url])
        .arg(dest_dir)
        .status()?;

    if !status.success() {
        return Err(io::Error::other(format!(
            "Failed to clone git repository: {}",
            url
        )));
    }

    eprintln!("Done!");
    Ok(())
}

/// Download all test suites.
pub fn download_all(dest_dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dest_dir)?;

    for suite in TEST_SUITES {
        let suite_path = dest_dir.join(suite.name);
        if suite_path.exists() {
            eprintln!(
                "Skipping {} (already exists at {})",
                suite.name,
                suite_path.display()
            );
            continue;
        }

        if let Err(e) = download_test_suite(suite, dest_dir) {
            eprintln!("Warning: Failed to download {}: {}", suite.name, e);
        }
    }

    Ok(())
}

/// List available test suites.
pub fn list_suites() {
    eprintln!("Available test suites:");
    for suite in TEST_SUITES {
        eprintln!("  {} - {}", suite.name, suite.description);
        eprintln!("    URL: {}", suite.url);
    }
}

use std::io::Write;

use anyhow::Result;
use include_dir::{include_dir, Dir};

use crate::exec::{self};

static DEFAULT_PACKAGES: &str = "bash ca-certificates apt net-tools iputils-ping procps vim bpftool";

static PROJECT_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/assets");

fn extract_file(filename: &str, target: &str) -> Result<()> {
    if let Some(asset) = PROJECT_DIR.get_file(filename) {
        let mut file = std::fs::File::create(target)?;
        file.write_all(asset.contents())?;
    }
    Ok(())
}

fn remove_dir(dir: impl Into<String>) {
    // just ignore the result!
    let _ = std::fs::remove_dir_all(dir.into());
}

// convert package string to a list of packages
fn convert_packages(packages: &str) -> Vec<String> {
    packages
        .split_whitespace()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
}

// collect packages from assets/packages/*
fn collect_packages(packages: &str) -> Result<Vec<String>> {
    if let Some(f) = PROJECT_DIR.get_file(&format!("packages/{}", packages)) {
        if let Some(contents) = f.contents_utf8() {
            return Ok(convert_packages(contents));
        }
    }
    Err(anyhow::anyhow!("packages not found: {}", packages))
}

// packages list to qemu-debootstrap command line --include
fn packages_to_cmdline(packages: Vec<String>) -> String {
    packages.join(",")
}

pub fn build(
    tempdir: Option<String>,
    arch: String,
    distro: String,
    bcc: bool,
    mirror: String,
) -> Result<()> {
    let info = os_info::get();
    let os_type = info.os_type();

    match os_type {
        os_info::Type::Ubuntu | os_info::Type::Debian => {}
        _ => {
            return Err(anyhow::anyhow!(
                "OS: {} is not supported, please build it in Ubuntu/Debian",
                os_type
            ))
        }
    }

    if which::which("qemu-debootstrap").is_err() {
        return Err(anyhow::anyhow!("qemu-debootstrap is not available, please try `sudo apt-get install qemu-user-static debootstrap`"));
    }

    let working_dir = tempfile::tempdir()?;

    let working_path = if let Some(dir) = tempdir {
        // delete our created tempdir
        std::mem::drop(working_dir);

        dir
    } else {
        working_dir.path().to_string_lossy().to_string()
    };

    let mut packages = convert_packages(DEFAULT_PACKAGES);

    if bcc {
        let mut bcc_packages = collect_packages("bcc")?;
        packages.append(&mut bcc_packages);
    }

    let packages = packages_to_cmdline(packages);
    let variant = "--variant=minbase";
    let out_dir = format!("{}/{}", working_path, "debian");
    let build_cmd = format!("time qemu-debootstrap --arch {arch} --include=\"{packages}\" {variant} {distro} {out_dir} {mirror}");

    // build the image
    exec::check_call(build_cmd)?;

    // make bash the default shell to make /usr/bin/apt-get work
    exec::check_call(format!("chroot {out_dir} rm /bin/sh"))?;
    exec::check_call(format!("chroot {out_dir} ln -s /bin/bash /bin/sh"))?;
    extract_file("bashrc", &format!("{}/.bashrc", out_dir))?;
    extract_file("get_kvers.sh", &format!("{}/get_kvers.sh", out_dir))?;

    // cleanup
    remove_dir(format!("{out_dir}/lib/udev/"));
    remove_dir(format!("{out_dir}/var/lib/apt/lists/"));
    remove_dir(format!("{out_dir}/var/cache/apt/archives/"));
    remove_dir(format!("{out_dir}/usr/share/locale/"));
    remove_dir(format!("{out_dir}/usr/lib/share/locale/"));
    remove_dir(format!("{out_dir}/usr/share/doc/"));
    remove_dir(format!("{out_dir}/usr/lib/share/doc/"));
    remove_dir(format!("{out_dir}/usr/share/ieee-data/"));
    remove_dir(format!("{out_dir}/usr/lib/share/ieee-data/"));
    remove_dir(format!("{out_dir}/usr/share/man/"));
    remove_dir(format!("{out_dir}/usr/lib/share/man/"));

    let dns = "4.2.2.2";
    // Add a default DNS server
    exec::call(format!(
        "echo \"nameserver {dns}\" > {out_dir}/etc/resolv.conf"
    ));

    // build tar
    let cmd = format!("tar -zcf deb.tar.gz -C {working_path} debian");
    exec::check_call(cmd)?;

    Ok(())
}

use anyhow::bail;
use log::{debug, info, trace};
use thiserror::Error;

use crate::*;
use crate::{
    commands::{mk_temp, run_command, run_command_capture, NHRunnable},
    interface::{FlakeRef, HomeArgs, HomeRebuildArgs, HomeSubcommand},
};

#[derive(Error, Debug)]
enum HomeRebuildError {
    #[error("Configuration \"{0}\" doesn't exist")]
    ConfigName(String),
    #[error("No confirmation")]
    NoConfirm,
}

impl NHRunnable for HomeArgs {
    fn run(&self) -> anyhow::Result<()> {
        // self.subcommand
        match &self.subcommand {
            HomeSubcommand::Switch(args) => args.rebuild(),
            s => bail!("Subcommand {:?} not yet implemented", s),
        }
    }
}

impl HomeRebuildArgs {
    fn rebuild(&self) -> anyhow::Result<()> {
        let out_dir = tempfile::Builder::new().prefix("nh-home-").tempdir()?;
        let out_link = out_dir.path().join("result");
        let out_link = out_link.to_str().unwrap();

        debug!("out_dir: {:?}", out_dir);
        debug!("out_link {:?}", out_link);

        let username = std::env::var("USER").expect("Couldn't get username");

        let hm_config_name = match &self.configuration {
            Some(name) => {
                if configuration_exists(&self.flakeref, name)? {
                    name.to_owned()
                } else {
                    return Err(HomeRebuildError::ConfigName(name.to_owned()).into());
                }
            }
            None => get_home_output(&self.flakeref, &username)?,
        };

        trace!("hm_config_name: {}", hm_config_name);

        let flakeref = format!(
            "{}#homeConfigurations.{}.config.home.activationPackage",
            &self.flakeref, hm_config_name
        );

        let build_cmd = commands::BuildCommandBuilder::default()
            .flakeref(&flakeref)
            .extra_args(self.extra_args.clone())
            .extra_args(vec!["--out-link".to_owned(), out_link.to_owned()])
            .build()?;

        debug!("{:?}", build_cmd);

        build_cmd.run()?;

        let previous_gen = format!("/nix/var/nix/profiles/per-user/{}/home-manager", &username);

        let diff_cmd = commands::CommandBuilder::default()
            .args(vec![
                "nvd".to_owned(),
                "diff".to_owned(),
                previous_gen,
                out_link.to_owned(),
            ])
            .build()?;

        debug!("diff_cmd: {:?}", diff_cmd);

        diff_cmd.run()?;

        if self.ask {
            info!("Apply the config?");
            let confirmation = dialoguer::Confirm::new().default(false).interact()?;

            if !confirmation {
                return Err(HomeRebuildError::NoConfirm.into());
            }
        }

        let activator = format!("{}/activate", out_link);
        let activation_cmd = commands::CommandBuilder::default()
            .args(vec![activator])
            .build()?;

        debug!("{:?}", activation_cmd);

        activation_cmd.run()?;

        // Drop the out dir *only* when we are finished
        drop(out_dir);

        Ok(())
    }
}

fn home_info() -> anyhow::Result<()> {
    Ok(())
}

fn get_home_output<S: AsRef<str> + std::fmt::Display>(
    flakeref: &FlakeRef,
    username: S,
) -> Result<String, subprocess::PopenError> {
    // Replicate these heuristics
    // https://github.com/nix-community/home-manager/blob/433e8de330fd9c157b636f9ccea45e3eeaf69ad2/home-manager/home-manager#L110

    let hostname = hostname::get()
        .expect("Couldn't get hostname")
        .into_string()
        .unwrap();

    let full_flakef = format!("{}@{}", username, &hostname);

    if configuration_exists(flakeref, &full_flakef)? {
        Ok(full_flakef)
    } else {
        Ok(username.to_string())
    }
}

fn configuration_exists(
    flakeref: &FlakeRef,
    configuration: &str,
) -> Result<bool, subprocess::PopenError> {
    let output = format!("{}#homeConfigurations", flakeref);
    let filter = format!(r#" x: x ? "{}" "#, configuration);

    let cmd_check = vec!["nix", "eval", &output, "--apply", &filter];

    run_command_capture(&cmd_check, None).map(|s| match s.trim() {
        "true" => true,
        "false" => false,
        _ => todo!(),
    })
}

use std::ffi::OsString;
use std::result::Result;

use comedy::bstr::BStr;
use comedy::com::InitCom;
use comedy::process::current_process_image_name;
use failure::{bail, Error};

use task_service::{InstancesPolicy, TaskService};

pub fn task_name() -> OsString {
    OsString::from("FOOOO")
}

pub fn install() -> Result<(), Error> {
    let _com_inited = InitCom::init_sta()?;

    // TODO name from install path hash
    let task_name = task_name();
    let command_line = OsString::from("ondemand $(Arg0)");

    // TODO this should be an arg?
    let image_path = current_process_image_name()?;

    let mut service = TaskService::connect_local()?;
    let mut root_folder = service.get_folder("\\")?;

    // Try to remove an existing task of the same name. Allowed to fail silently.
    let _delete_result = root_folder.delete_task(task_name.as_os_str());

    let mut task_def = service.new_task_definition()?;

    {
        task_def.get_registration_info()?.set_author("Mozilla")?;

        let mut settings = task_def.get_settings()?;
        settings.set_multiple_instances(InstancesPolicy::Parallel)?;
        settings.set_allow_demand_start(true)?;
        settings.set_run_only_if_idle(false)?;
        settings.set_disallow_start_if_on_batteries(false)?;
        settings.set_stop_if_going_on_batteries(false)?;
        settings.get_idle_settings()?.set_stop_on_idle_end(false)?;

        let mut action_collection = task_def.get_actions()?;
        let mut exec_action = action_collection.create_exec()?;
        exec_action.set_path(image_path.as_os_str())?;
        exec_action.set_arguments(command_line.as_os_str())?;
    }

    let mut registered_task =
        root_folder.create_task_local_service(&task_def, Some(task_name.as_os_str()))?;

    // Allow read and execute access by builtin users, this is required to Get the task and
    // call Run on it
    // TODO: should this just be in sddl above? I think that ends up adding BU as principal?
    registered_task.set_sd_dont_add_principal_ace("D:(A;;GRGX;;;BU)")?;

    Ok(())
}

pub fn uninstall() -> Result<(), Error> {
    let _com_inited = InitCom::init_sta()?;

    let task_name = task_name();

    TaskService::connect_local()?
        .get_folder("\\")?
        .delete_task(task_name.as_os_str())?;

    Ok(())
}

pub fn run_on_demand<T, U>(task_name: T, arg: U) -> Result<(), Error>
where
    T: Into<BStr>,
    U: Into<BStr>,
{
    let _com_inited = InitCom::init_sta()?;

    let maybe_task = TaskService::connect_local()?
        .get_folder("\\")?
        .get_task(task_name)?;
    let mut task = if let Some(task) = maybe_task {
        task
    } else {
        bail!("No such task");
    };

    let _running_task = task.run(arg)?;

    Ok(())
}

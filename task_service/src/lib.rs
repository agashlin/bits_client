extern crate comedy;
extern crate winapi;
extern crate wio;

#[rustfmt::skip]
mod taskschd;

use std::ptr;
use std::result;

use comedy::bstr::BStr;
use comedy::com::{cast, create_instance_inproc_server};
use comedy::error::{ErrorCode::*, ResultExt as ComicalResultExt};
use comedy::variant::{variant_bool, Variant};
use comedy::{com_call, com_call_getter, Error};

use taskschd::{
    IActionCollection, IExecAction, IIdleSettings, IRegisteredTask, IRegistrationInfo,
    IRunningTask, ITaskDefinition, ITaskFolder, ITaskService, ITaskSettings, TaskScheduler,
    TASK_ACTION_EXEC, TASK_CREATE, TASK_DONT_ADD_PRINCIPAL_ACE, TASK_INSTANCES_IGNORE_NEW,
    TASK_INSTANCES_PARALLEL, TASK_INSTANCES_QUEUE, TASK_INSTANCES_STOP_EXISTING,
    TASK_LOGON_SERVICE_ACCOUNT,
};
use winapi::shared::{
    minwindef::DWORD,
    ntdef::LONG,
    winerror::{ERROR_FILE_NOT_FOUND, HRESULT, HRESULT_FROM_WIN32},
};
use wio::com::ComPtr;

type Result<T> = result::Result<T, Error>;

pub struct TaskService(ComPtr<ITaskService>);
impl TaskService {
    pub fn connect_local() -> Result<TaskService> {
        let task_service = create_instance_inproc_server::<TaskScheduler, ITaskService>()?;

        // Connect to local service with no credentials.
        unsafe {
            let null = Variant::null().get();
            com_call!(task_service, ITaskService::Connect(null, null, null, null))?;
        }

        Ok(TaskService(task_service))
    }

    pub fn get_folder<T>(&mut self, path: T) -> Result<TaskFolder>
    where
        T: Into<BStr>,
    {
        let path: BStr = path.into();
        Ok(TaskFolder(unsafe {
            com_call_getter!(|folder| self.0, ITaskService::GetFolder(path.get(), folder))
        }?))
    }

    pub fn new_task_definition(&mut self) -> Result<TaskDefinition> {
        Ok(TaskDefinition(unsafe {
            com_call_getter!(
                |task_def| self.0,
                ITaskService::NewTask(
                    0, // flags (reserved)
                    task_def,
                )
            )
        }?))
    }
}

pub struct RunningTask(ComPtr<IRunningTask>);

pub struct TaskFolder(ComPtr<ITaskFolder>);
impl TaskFolder {
    pub fn delete_task<T>(&mut self, path: T) -> Result<HRESULT>
    where
        T: Into<BStr>,
    {
        let path: BStr = path.into();
        unsafe {
            com_call!(
                self.0,
                ITaskFolder::DeleteTask(
                    path.get(),
                    0 // flags
                )
            )
        }
    }

    pub fn get_task<T>(&mut self, path: T) -> Result<Option<RegisteredTask>>
    where
        T: Into<BStr>,
    {
        let path: BStr = path.into();
        let task =
            unsafe { com_call_getter!(|task| self.0, ITaskFolder::GetTask(path.get(), task)) };

        // Return Ok(None) if not found
        Ok(task
            .map(|t| Some(t))
            .allow_err(HResult(HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)), None)?
            .map(|t| RegisteredTask(t)))
    }

    pub fn create_task_local_service<T>(
        &mut self,
        definition: &TaskDefinition,
        path: Option<T>,
    ) -> Result<RegisteredTask>
    where
        T: Into<BStr>,
    {
        let path: Option<BStr> = path.map(|p| p.into());
        let account = BStr::from("NT AUTHORITY\\LocalService");
        let account = Variant::wrap(&account);
        let password = Variant::null();
        let empty_sddl = BStr::empty();
        let empty_sddl: Variant<BStr> = Variant::wrap(&empty_sddl);

        Ok(RegisteredTask(unsafe {
            com_call_getter!(
                |rt| self.0,
                ITaskFolder::RegisterTaskDefinition(
                    if let Some(path) = path {
                        path.get()
                    } else {
                        ptr::null_mut()
                    },
                    definition.0.as_raw(),
                    TASK_CREATE as LONG,
                    account.get(),
                    password.get(),
                    TASK_LOGON_SERVICE_ACCOUNT,
                    empty_sddl.get(), // sddl
                    rt,
                )
            )
        }?))
    }
}

pub struct RegisteredTask(ComPtr<IRegisteredTask>);
impl RegisteredTask {
    pub fn set_sd_dont_add_principal_ace<T>(&mut self, sddl: T) -> Result<HRESULT>
    where
        T: Into<BStr>,
    {
        let sddl: BStr = sddl.into();
        unsafe {
            com_call!(
                self.0,
                IRegisteredTask::SetSecurityDescriptor(
                    sddl.get(),
                    TASK_DONT_ADD_PRINCIPAL_ACE as LONG,
                )
            )
        }
    }

    pub fn run<T>(&mut self, param: T) -> Result<RunningTask>
    where
        T: Into<BStr>,
    {
        let param = param.into();
        let param = Variant::<BStr>::wrap(&param);
        Ok(RunningTask(unsafe {
            com_call_getter!(|rt| self.0, IRegisteredTask::Run(param.get(), rt))
        }?))
    }
}

pub struct RegistrationInfo(ComPtr<IRegistrationInfo>);
impl RegistrationInfo {
    pub fn set_author<'a, T>(&mut self, name: T) -> Result<HRESULT>
    where
        T: Into<BStr>,
    {
        let name: BStr = name.into();
        unsafe { com_call!(self.0, IRegistrationInfo::put_Author(name.get(),)) }
    }
}

pub struct TaskDefinition(ComPtr<ITaskDefinition>);
impl TaskDefinition {
    pub fn get_registration_info(&mut self) -> Result<RegistrationInfo> {
        Ok(RegistrationInfo(unsafe {
            com_call_getter!(|info| self.0, ITaskDefinition::get_RegistrationInfo(info))
        }?))
    }

    pub fn get_settings(&mut self) -> Result<TaskSettings> {
        Ok(TaskSettings(unsafe {
            com_call_getter!(|s| self.0, ITaskDefinition::get_Settings(s))
        }?))
    }

    pub fn get_actions(&mut self) -> Result<ActionCollection> {
        Ok(ActionCollection(unsafe {
            com_call_getter!(|ac| self.0, ITaskDefinition::get_Actions(ac))
        }?))
    }
}

#[repr(u32)]
pub enum InstancesPolicy {
    Parallel = TASK_INSTANCES_PARALLEL,
    Queue = TASK_INSTANCES_QUEUE,
    IgnoreNew = TASK_INSTANCES_IGNORE_NEW,
    StopExisting = TASK_INSTANCES_STOP_EXISTING,
}

pub struct TaskSettings(ComPtr<ITaskSettings>);
impl TaskSettings {
    pub fn set_multiple_instances(&mut self, policy: InstancesPolicy) -> Result<HRESULT> {
        unsafe {
            com_call!(
                self.0,
                ITaskSettings::put_MultipleInstances(policy as DWORD)
            )
        }
    }

    pub fn set_allow_demand_start(&mut self, v: bool) -> Result<HRESULT> {
        unsafe { com_call!(self.0, ITaskSettings::put_AllowDemandStart(variant_bool(v))) }
    }

    pub fn set_run_only_if_idle(&mut self, v: bool) -> Result<HRESULT> {
        unsafe { com_call!(self.0, ITaskSettings::put_RunOnlyIfIdle(variant_bool(v))) }
    }

    pub fn set_disallow_start_if_on_batteries(&mut self, v: bool) -> Result<HRESULT> {
        unsafe {
            com_call!(
                self.0,
                ITaskSettings::put_DisallowStartIfOnBatteries(variant_bool(v))
            )
        }
    }

    pub fn set_stop_if_going_on_batteries(&mut self, v: bool) -> Result<HRESULT> {
        unsafe {
            com_call!(
                self.0,
                ITaskSettings::put_StopIfGoingOnBatteries(variant_bool(v))
            )
        }
    }

    pub fn get_idle_settings(&mut self) -> Result<IdleSettings> {
        Ok(IdleSettings(unsafe {
            com_call_getter!(|is| self.0, ITaskSettings::get_IdleSettings(is))
        }?))
    }
}

pub struct IdleSettings(ComPtr<IIdleSettings>);
impl IdleSettings {
    pub fn set_stop_on_idle_end(&mut self, v: bool) -> Result<HRESULT> {
        unsafe { com_call!(self.0, IIdleSettings::put_StopOnIdleEnd(variant_bool(v))) }
    }
}

pub struct ActionCollection(ComPtr<IActionCollection>);
impl ActionCollection {
    pub fn create_exec(&mut self) -> Result<ExecAction> {
        let action = unsafe {
            com_call_getter!(|a| self.0, IActionCollection::Create(TASK_ACTION_EXEC, a))
        }?;

        Ok(ExecAction(cast(action)?))
    }
}

pub struct ExecAction(ComPtr<IExecAction>);
impl ExecAction {
    pub fn set_path<T>(&mut self, path: T) -> Result<HRESULT>
    where
        T: Into<BStr>,
    {
        unsafe { com_call!(self.0, IExecAction::put_Path(path.into().get())) }
    }

    pub fn set_arguments<T>(&mut self, arguments: T) -> Result<HRESULT>
    where
        T: Into<BStr>,
    {
        unsafe { com_call!(self.0, IExecAction::put_Arguments(arguments.into().get())) }
    }
}

use std::borrow::Cow;
use std::ffi::OsStr;
use std::{io, ptr};

use widestring::{NulError, WideCString, WideString};
use winapi::um::winsvc;
use winapi::um::winnt;
use winapi::shared::winerror;

use sc_handle::ScHandle;
use service::{Service, ServiceAccess, ServiceInfo};
use shell_escape;

use {ErrorKind, Result, ResultExt};
use widestring::WideCStr;
use std;
use service::*;
use std::mem;
use service::EnumListServiceResult;

bitflags! {
    /// Flags describing access permissions for [`ServiceManager`].
    pub struct ServiceManagerAccess: u32 {
        /// Can connect to service control manager.
        const CONNECT = winsvc::SC_MANAGER_CONNECT;

        /// Can create services.
        const CREATE_SERVICE = winsvc::SC_MANAGER_CREATE_SERVICE;

        /// Can enumerate services or receive notifications.
        const ENUMERATE_SERVICE = winsvc::SC_MANAGER_ENUMERATE_SERVICE;
    }
}

/// Service manager.
pub struct ServiceManager {
    manager_handle: ScHandle,
}

impl ServiceManager {
    /// Private initializer.
    ///
    /// # Arguments
    ///
    /// * `machine`  - The name of machine.
    ///                Pass `None` to connect to local machine.
    /// * `database` - The name of database to connect to.
    ///                Pass `None` to connect to active database.
    ///
    fn new<M: AsRef<OsStr>, D: AsRef<OsStr>>(
        machine: Option<M>,
        database: Option<D>,
        request_access: ServiceManagerAccess,
    ) -> Result<Self> {
        let machine_name = to_wide(machine).chain_err(|| ErrorKind::InvalidMachineName)?;
        let database_name = to_wide(database).chain_err(|| ErrorKind::InvalidDatabaseName)?;
        let handle = unsafe {
            winsvc::OpenSCManagerW(
                machine_name.map_or(ptr::null(), |s| s.as_ptr()),
                database_name.map_or(ptr::null(), |s| s.as_ptr()),
                request_access.bits(),
            )
        };

        if handle.is_null() {
            Err(io::Error::last_os_error().into())
        } else {
            Ok(ServiceManager {
                manager_handle: unsafe { ScHandle::new(handle) },
            })
        }
    }

    /// Connect to local services database.
    ///
    /// # Arguments
    ///
    /// * `database`       - The name of database to connect to.
    ///                      Pass `None` to connect to active database.
    /// * `request_access` - Desired access permissions.
    ///
    pub fn local_computer<D: AsRef<OsStr>>(
        database: Option<D>,
        request_access: ServiceManagerAccess,
    ) -> Result<Self> {
        ServiceManager::new(None::<&OsStr>, database, request_access)
    }

    /// Connect to remote services database.
    ///
    /// # Arguments
    ///
    /// * `machine`        - The name of remote machine.
    /// * `database`       - The name of database to connect to.
    ///                      Pass `None` to connect to active database.
    /// * `request_access` - desired access permissions.
    ///
    pub fn remote_computer<M: AsRef<OsStr>, D: AsRef<OsStr>>(
        machine: M,
        database: Option<D>,
        request_access: ServiceManagerAccess,
    ) -> Result<Self> {
        ServiceManager::new(Some(machine), database, request_access)
    }

    /// Create a service.
    ///
    /// # Arguments
    ///
    /// * `service_info`   - The service information that will be saved to the system services
    ///                      registry.
    /// * `service_access` - Desired access permissions for the returned [`Service`]
    ///                      instance.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use std::ffi::OsString;
    /// use std::path::PathBuf;
    /// use windows_service::service::{
    ///     ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceType,
    /// };
    /// use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
    ///
    /// fn main() -> windows_service::Result<()> {
    ///     let manager =
    ///         ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CREATE_SERVICE)?;
    ///
    ///     let my_service_info = ServiceInfo {
    ///         name: OsString::from("my_service"),
    ///         display_name: OsString::from("My service"),
    ///         service_type: ServiceType::OwnProcess,
    ///         start_type: ServiceStartType::OnDemand,
    ///         error_control: ServiceErrorControl::Normal,
    ///         executable_path: PathBuf::from(r"C:\path\to\my\service.exe"),
    ///         launch_arguments: vec![],
    ///         account_name: None, // run as System
    ///         account_password: None,
    ///     };
    ///
    ///     let my_service = manager.create_service(my_service_info, ServiceAccess::QUERY_STATUS)?;
    ///     Ok(())
    /// }
    /// ```
    pub fn create_service(
        &self,
        service_info: ServiceInfo,
        service_access: ServiceAccess,
    ) -> Result<Service> {
        let service_name =
            WideCString::from_str(service_info.name).chain_err(|| ErrorKind::InvalidServiceName)?;
        let display_name = WideCString::from_str(service_info.display_name)
            .chain_err(|| ErrorKind::InvalidDisplayName)?;
        let account_name =
            to_wide(service_info.account_name).chain_err(|| ErrorKind::InvalidAccountName)?;
        let account_password =
            to_wide(service_info.account_password).chain_err(|| ErrorKind::InvalidAccountPassword)?;

        // escape executable path and arguments and combine them into single command
        let executable_path = match service_info.service_type {
            ServiceType::KernelDriver => Ok(WideString::from_str(&service_info.executable_path)),
            _ => escape_wide(service_info.executable_path)
        }.chain_err(|| ErrorKind::InvalidExecutablePath)?;

        let mut launch_command_buffer = WideString::new();
        launch_command_buffer.push(executable_path);

        for launch_argument in service_info.launch_arguments.iter() {
            let wide = escape_wide(launch_argument).chain_err(|| ErrorKind::InvalidLaunchArgument)?;

            launch_command_buffer.push_str(" ");
            launch_command_buffer.push(wide);
        }

        let launch_command = WideCString::from_wide_str(launch_command_buffer).unwrap();

        let service_handle = unsafe {
            winsvc::CreateServiceW(
                self.manager_handle.raw_handle(),
                service_name.as_ptr(),
                display_name.as_ptr(),
                service_access.bits(),
                service_info.service_type.to_raw(),
                service_info.start_type.to_raw(),
                service_info.error_control.to_raw(),
                launch_command.as_ptr(),
                ptr::null(),     // load ordering group
                ptr::null_mut(), // tag id within the load ordering group
                ptr::null(),     // service dependencies
                account_name.map_or(ptr::null(), |s| s.as_ptr()),
                account_password.map_or(ptr::null(), |s| s.as_ptr()),
            )
        };

        if service_handle.is_null() {
            Err(io::Error::last_os_error().into())
        } else {
            Ok(Service::new(unsafe { ScHandle::new(service_handle) }))
        }
    }

    /// Open an existing service.
    ///
    /// # Arguments
    ///
    /// * `name`           - The service name.
    /// * `request_access` - Desired permissions for the returned [`Service`] instance.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use windows_service::service::ServiceAccess;
    /// use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
    ///
    /// # fn main() -> windows_service::Result<()> {
    /// let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    /// let my_service = manager.open_service("my_service", ServiceAccess::QUERY_STATUS)?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    pub fn open_service<T: AsRef<OsStr>>(
        &self,
        name: T,
        request_access: ServiceAccess,
    ) -> Result<Service> {
        let service_name = WideCString::from_str(name).chain_err(|| ErrorKind::InvalidServiceName)?;
        let service_handle = unsafe {
            winsvc::OpenServiceW(
                self.manager_handle.raw_handle(),
                service_name.as_ptr(),
                request_access.bits(),
            )
        };

        if service_handle.is_null() {
            Err(io::Error::last_os_error().into())
        } else {
            Ok(Service::new(unsafe { ScHandle::new(service_handle) }))
        }
    }

    pub fn list_services(&self) -> Result<Vec<ServiceDetail>> {

        let mut service_list: Vec<ServiceDetail> = vec![];

        let mut pcb_bytes_needed = 0;
        let mut lp_services_returned = 0;
        let mut lp_resume_handle = 0;
        unsafe {
            winsvc::EnumServicesStatusExW(self.manager_handle.raw_handle(),
                                          winsvc::SC_ENUM_PROCESS_INFO,
                                  winnt::SERVICE_WIN32 | winnt::SERVICE_DRIVER,//SERVICE_TYPE_ALL,
                                  winsvc::SERVICE_STATE_ALL,
                                  std::ptr::null_mut(),
                                  0,
                                  &mut pcb_bytes_needed,
                                  &mut lp_services_returned,
                                  &mut lp_resume_handle,
                                  std::ptr::null(),
            )
        };

        let last_error = io::Error::last_os_error().raw_os_error().unwrap_or(0);

        if winerror::ERROR_MORE_DATA as i32 == last_error {
            let mut lp_services = vec![unsafe { std::mem::uninitialized() }; pcb_bytes_needed as usize];

           unsafe {
                winsvc::EnumServicesStatusExW(self.manager_handle.raw_handle(),
                                              winsvc::SC_ENUM_PROCESS_INFO,
                                              winnt::SERVICE_WIN32 | winnt::SERVICE_DRIVER,//SERVICE_TYPE_ALL,
                                              winsvc::SERVICE_STATE_ALL,
                                      lp_services.as_mut_ptr(),
                                      pcb_bytes_needed,
                                      &mut pcb_bytes_needed,
                                      &mut lp_services_returned,
                                      &mut lp_resume_handle,
                                      std::ptr::null(),
                )
            };

            let enum_result = EnumListServiceResult::from_raw(lp_services.as_slice().as_ptr(), lp_services_returned);
            for service_status in enum_result
                {
                    let handle_service = unsafe { winsvc::OpenServiceW(self.manager_handle.raw_handle(),
                                                              service_status.lpServiceName,
                                                              winsvc::SC_MANAGER_ALL_ACCESS) };

                    let mut pcb_bytes_needed = 0;
                    unsafe { winsvc::QueryServiceConfigW(handle_service, std::ptr::null_mut(), 0, &mut pcb_bytes_needed) };

                    if pcb_bytes_needed > 0 {
                        let mut tmp = vec![0u8; pcb_bytes_needed as usize];
                        let query_service_config: *mut winsvc::QUERY_SERVICE_CONFIGW = unsafe { mem::transmute(tmp.as_mut_ptr()) };

                        unsafe { winsvc::QueryServiceConfigW(handle_service, query_service_config, pcb_bytes_needed + 0, &mut pcb_bytes_needed) };

                        let service_detail = unsafe { ServiceDetail {
                            status: ServiceStatusExt::from_raw(service_status.ServiceStatusProcess)?,
                            name: WideCStr::from_ptr_str(service_status.lpServiceName).to_string_lossy(),
                            display_name: WideCStr::from_ptr_str(service_status.lpDisplayName).to_string_lossy(),
                            binary_path: Some(WideCStr::from_ptr_str((*query_service_config).lpBinaryPathName).to_string_lossy()),
                            start_type: Some(ServiceStartType::from_raw((*query_service_config).dwStartType)?),
                            error_control: Some(ServiceErrorControl::from_raw((*query_service_config).dwErrorControl)?),
                            tag_id: Some((*query_service_config).dwErrorControl),
                            start_name: Some(WideCStr::from_ptr_str((*query_service_config).lpServiceStartName).to_string_lossy()),
                            load_order_group: Some(WideCStr::from_ptr_str((*query_service_config).lpLoadOrderGroup).to_string_lossy()),
                            dependencies: Some(WideCStr::from_ptr_str((*query_service_config).lpDependencies).to_string_lossy())

                        }};

                        service_list.push(service_detail);
                    } else {

                        let service_detail = unsafe { ServiceDetail {
                            status: ServiceStatusExt::from_raw(service_status.ServiceStatusProcess)?,
                            name: WideCStr::from_ptr_str(service_status.lpServiceName).to_string_lossy(),
                            display_name: WideCStr::from_ptr_str(service_status.lpDisplayName).to_string_lossy(),
                            binary_path: Some(format!("Error when retrieving info for service {}", io::Error::last_os_error())),
                            start_type: None,
                            error_control: None,
                            tag_id: None,
                            start_name: None,
                            load_order_group: None,
                            dependencies: None

                        }};

                        service_list.push(service_detail);
                    }
                    unsafe { winsvc::CloseServiceHandle(handle_service)};
                }
        }

        Ok(service_list)
    }
}

fn to_wide<T: AsRef<OsStr>>(s: Option<T>) -> ::std::result::Result<Option<WideCString>, NulError> {
    if let Some(s) = s {
        Ok(Some(WideCString::from_str(s)?))
    } else {
        Ok(None)
    }
}

fn escape_wide<T: AsRef<OsStr>>(s: T) -> ::std::result::Result<WideString, NulError> {
    let escaped = shell_escape::escape(Cow::Borrowed(s.as_ref()));
    let wide = WideCString::from_str(escaped)?;
    Ok(wide.to_wide_string())
}

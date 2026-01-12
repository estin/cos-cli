use cosmic_protocols::toplevel_info::v1::client::{
    zcosmic_toplevel_handle_v1, zcosmic_toplevel_info_v1,
};
use cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1;

use wayland_client::{
    Connection, Dispatch, QueueHandle, event_created_child,
    protocol::{wl_output, wl_registry},
};
use wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_group_handle_v1, ext_workspace_handle_v1, ext_workspace_manager_v1,
};

use crate::{App, AppState};

impl Dispatch<wl_registry::WlRegistry, ()> for AppState {
    fn event(
        state: &mut Self,
        proxy: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_output" => {
                    proxy.bind::<wl_output::WlOutput, _, _>(name, 4, qh, ());
                }
                "zcosmic_toplevel_info_v1" => {
                    proxy.bind::<zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1, _, _>(
                        name,
                        1,
                        qh,
                        (),
                    );
                }
                "ext_workspace_manager_v1" => {
                    proxy.bind::<ext_workspace_manager_v1::ExtWorkspaceManagerV1, _, _>(
                        name,
                        version,
                        qh,
                        (),
                    );
                }
                "zcosmic_toplevel_manager_v1" => {
                    state.cosmic_toplevel_manager = Some(
                        proxy.bind::<zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1, _, _>(
                            name,
                            version,
                            qh,
                            (),
                        ),
                    );
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for AppState {
    fn event(
        app_data: &mut Self,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<AppState>,
    ) {
        if let wl_output::Event::Name { name } = event {
            app_data.outputs.push((output.clone(), name));
        }
    }
}

// Implement Dispatch for the workspace manager to handle events like 'Workspace created'
impl Dispatch<ext_workspace_manager_v1::ExtWorkspaceManagerV1, ()> for AppState {
    fn event(
        _state: &mut Self,
        _proxy: &ext_workspace_manager_v1::ExtWorkspaceManagerV1,
        _event: ext_workspace_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }

    event_created_child!(
        AppState,
        ext_workspace_manager_v1::ExtWorkspaceManagerV1,
        [
            ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ext_workspace_handle_v1::ExtWorkspaceHandleV1, ()),
            ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1, ()),
        ]
    );
}

impl Dispatch<ext_workspace_handle_v1::ExtWorkspaceHandleV1, ()> for AppState {
    fn event(
        state: &mut Self,
        proxy: &ext_workspace_handle_v1::ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let ext_workspace_handle_v1::Event::Name { name } = event {
            state.workspaces.push((name, proxy.clone()));
        }
    }
}

impl Dispatch<ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1, ()> for AppState {
    fn event(
        _state: &mut Self,
        _proxy: &ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1,
        _event: ext_workspace_group_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1, ()> for AppState {
    fn event(
        _app_data: &mut AppState,
        _workspace: &zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1,
        _event: zcosmic_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1, ()> for AppState {
    fn event(
        app_data: &mut Self,
        _info: &zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1,
        event: zcosmic_toplevel_info_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<AppState>,
    ) {
        if let zcosmic_toplevel_info_v1::Event::Toplevel { toplevel } = event {
            app_data.apps.push(App {
                handle: toplevel,
                title: None,
                app_id: None,
                outputs: Vec::new(),
                // workspaces: Vec::new(),
                // state: Vec::new(),
            })
        }
    }

    event_created_child!(
        AppState ,
        zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1,
        [
            zcosmic_toplevel_info_v1::EVT_TOPLEVEL_OPCODE => (zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1, ()),
        ]
    );
}

impl Dispatch<zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1, ()> for AppState {
    fn event(
        app_data: &mut Self,
        toplevel: &zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1,
        event: zcosmic_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<AppState>,
    ) {
        match event {
            zcosmic_toplevel_handle_v1::Event::Title { title } => {
                if let Some(info) = app_data.apps.iter_mut().find(|t| &t.handle == toplevel) {
                    info.title = Some(title);
                }
            }
            zcosmic_toplevel_handle_v1::Event::AppId { app_id } => {
                if let Some(info) = app_data.apps.iter_mut().find(|t| &t.handle == toplevel) {
                    info.app_id = Some(app_id);
                }
            }
            zcosmic_toplevel_handle_v1::Event::OutputEnter { output } => {
                if let Some(info) = app_data.apps.iter_mut().find(|t| &t.handle == toplevel) {
                    info.outputs.push(output);
                }
            }
            // zcosmic_toplevel_handle_v1::Event::OutputLeave { output } => {
            //     if let Some(info) = app_data
            //         .toplevels
            //         .iter_mut()
            //         .find(|t| &t.handle == toplevel)
            //     {
            //         info.outputs.retain(|o| o != &output);
            //     }
            // }
            // zcosmic_toplevel_handle_v1::Event::WorkspaceEnter { workspace } => {
            //     if let Some(info) = app_data
            //         .toplevels
            //         .iter_mut()
            //         .find(|t| &t.handle == toplevel)
            //     {
            //         info.workspaces.push(workspace);
            //     }
            // }
            // zcosmic_toplevel_handle_v1::Event::WorkspaceLeave { workspace } => {
            //     if let Some(info) = app_data
            //         .toplevels
            //         .iter_mut()
            //         .find(|t| &t.handle == toplevel)
            //     {
            //         info.workspaces.retain(|w| w != &workspace);
            //     }
            // }
            // zcosmic_toplevel_handle_v1::Event::State { state } => {
            //     if let Some(info) = app_data
            //         .toplevels
            //         .iter_mut()
            //         .find(|t| &t.handle == toplevel)
            //     {
            //         info.state = state
            //             .chunks_exact(4)
            //             .map(|chunk| u32::from_ne_bytes(chunk.try_into().unwrap()))
            //             .flat_map(|val| State::try_from(val).ok())
            //             .collect::<Vec<_>>();
            //     }
            // }
            _ => {}
        }
    }
}

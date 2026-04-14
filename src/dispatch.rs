use cosmic_protocols::toplevel_info::v1::client::{
    zcosmic_toplevel_handle_v1, zcosmic_toplevel_info_v1,
};
use cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1;
use cosmic_protocols::workspace::v1::client::{
    zcosmic_workspace_group_handle_v1, zcosmic_workspace_manager_v1,
};
use cosmic_protocols::workspace::v2::client::{
    zcosmic_workspace_handle_v2, zcosmic_workspace_manager_v2,
};

use wayland_client::Proxy;
use wayland_client::protocol::wl_seat;
use wayland_client::{
    Connection, Dispatch, QueueHandle, event_created_child,
    protocol::{wl_output, wl_registry},
};
use wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_group_handle_v1, ext_workspace_handle_v1, ext_workspace_manager_v1,
};

use crate::{App, AppState, NamedHandle, State};

pub fn bind(proxy: &wl_registry::WlRegistry, qh: &QueueHandle<AppState>, state: &mut AppState) {
    if let Some((name, version)) = state.available_interfaces.get("ext_workspace_manager_v1") {
        proxy.bind::<ext_workspace_manager_v1::ExtWorkspaceManagerV1, _, _>(
            *name,
            *version,
            qh,
            (),
        );
    }
    if let Some((name, version)) = state
        .available_interfaces
        .get("zcosmic_workspace_manager_v1")
    {
        proxy.bind::<zcosmic_workspace_manager_v1::ZcosmicWorkspaceManagerV1, _, _>(
            *name,
            *version,
            qh,
            (),
        );
    }
    if let Some((name, version)) = state
        .available_interfaces
        .get("zcosmic_toplevel_manager_v1")
    {
        state.cosmic_toplevel_manager = Some(
            proxy.bind::<zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1, _, _>(
                *name,
                *version,
                qh,
                (),
            ),
        );
    }
    if let Some((name, version)) = state
        .available_interfaces
        .get("zcosmic_workspace_manager_v2")
    {
        proxy.bind::<zcosmic_workspace_manager_v2::ZcosmicWorkspaceManagerV2, _, _>(
            *name,
            *version,
            qh,
            (),
        );
    }
    if let Some((name, version)) = state
        .available_interfaces
        .get("zcosmic_workspace_handle_v2")
    {
        proxy.bind::<zcosmic_workspace_handle_v2::ZcosmicWorkspaceHandleV2, _, _>(
            *name,
            *version,
            qh,
            (),
        );
    }
    if let Some((name, version)) = state.available_interfaces.get("wl_output") {
        proxy.bind::<wl_output::WlOutput, _, _>(*name, *version, qh, ());
    }
    if let Some((name, version)) = state.available_interfaces.get("wl_seat") {
        proxy.bind::<wl_seat::WlSeat, _, _>(*name, *version, qh, ());
    }
    if let Some((name, _version)) = state.available_interfaces.get("zcosmic_toplevel_info_v1") {
        proxy.bind::<zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1, _, _>(*name, 1, qh, ());
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for AppState {
    fn event(
        state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            tracing::debug!(
                name = name,
                interface = interface,
                version = version,
                "WlRegistry Global",
            );
            state
                .available_interfaces
                .insert(interface.clone(), (name, version));
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
        tracing::debug!(event = ?event, output = ?output, "WlOutput");
        match event {
            wl_output::Event::Name { name } => {
                let output_id = output.id();
                app_data.handle_map.output.insert(
                    output_id.clone(),
                    NamedHandle::named(&name, output.to_owned()),
                );
                app_data.outputs.push(output_id);
            }
            wl_output::Event::Done => {
                app_data.discover_done = true;
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for AppState {
    fn event(
        app_data: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<AppState>,
    ) {
        tracing::debug!(event = ?event, seat = ?seat, "WlSeat");
        if let wl_seat::Event::Name { name } = event {
            let id = seat.id();
            app_data
                .handle_map
                .seat
                .insert(id.clone(), NamedHandle::named(&name, seat.to_owned()));
            app_data.seats.push(id)
        }
    }
}

impl Dispatch<ext_workspace_manager_v1::ExtWorkspaceManagerV1, ()> for AppState {
    fn event(
        state: &mut Self,
        proxy: &ext_workspace_manager_v1::ExtWorkspaceManagerV1,
        event: ext_workspace_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        tracing::debug!(event = ?event, proxy = ?proxy, "ExtWorkspaceManagerV1");
        if let ext_workspace_manager_v1::Event::WorkspaceGroup { workspace_group } = event {
            state.workspace_groups.push(crate::WorkspaceGroup {
                object_id: workspace_group.id(),
                workspaces: Vec::new(),
                outputs: Vec::new(),
            })
        }
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

// impl Dispatch<zcosmic_workspace_manager_v1::ZcosmicWorkspaceManagerV1, ()> for AppState {
//     fn event(
//         state: &mut Self,
//         proxy: &zcosmic_workspace_manager_v1::ZcosmicWorkspaceManagerV1,
//         event: zcosmic_workspace_manager_v1::Event,
//         _data: &(),
//         _conn: &Connection,
//         _qh: &QueueHandle<Self>,
//     ) {
//         tracing::debug!(event = ?event, proxy = ?proxy, "ZcosmicWorkspaceManagerV1");
//         // if let zcosmic_workspace_manager_v1::Event::WorkspaceGroup { workspace_group } = event {
//         //     state.cosmic_workspace_groups.push(CosmicWorkspaceGroup {
//         //         handle: workspace_group,
//         //         workspaces: Vec::new(),
//         //         outputs: Vec::new(),
//         //     })
//         // }
//     }

//     event_created_child!(
//         AppState,
//         zcosmic_workspace_manager_v1::ZcosmicWorkspaceManagerV1,
//         [
//             zcosmic_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (zcosmic_workspace_group_handle_v1::ZcosmicWorkspaceGroupHandleV1, ()),
//         ]
//     );
// }

impl Dispatch<ext_workspace_handle_v1::ExtWorkspaceHandleV1, ()> for AppState {
    fn event(
        state: &mut Self,
        proxy: &ext_workspace_handle_v1::ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        tracing::debug!(event = ?event, proxy = ?proxy, "ExtWorkspaceHandleV1");
        if let ext_workspace_handle_v1::Event::Name { name } = event {
            state
                .handle_map
                .workspace_handle
                .insert(proxy.id(), NamedHandle::named(&name, proxy.to_owned()));
        }
    }
}

impl Dispatch<ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1, ()> for AppState {
    fn event(
        state: &mut Self,
        proxy: &ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1,
        event: ext_workspace_group_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        tracing::debug!(event = ?event, proxy = ?proxy, "ExtWorkspaceGroupHandleV1");
        if let ext_workspace_group_handle_v1::Event::WorkspaceEnter { workspace } = event {
            let id = proxy.id();
            if let Some(group) = state
                .workspace_groups
                .iter_mut()
                .find(|g| g.object_id == id)
            {
                group.workspaces.push(workspace.id());
            } else {
                tracing::debug!("Workspace group not found")
            }
        }
    }
}

impl Dispatch<zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1, ()> for AppState {
    fn event(
        _app_data: &mut AppState,
        proxy: &zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1,
        event: zcosmic_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        tracing::debug!(event = ?event, proxy = ?proxy, "ZcosmicToplevelManagerV1");
    }
}

impl Dispatch<zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1, ()> for AppState {
    fn event(
        app_data: &mut Self,
        proxy: &zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1,
        event: zcosmic_toplevel_info_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<AppState>,
    ) {
        tracing::debug!(event = ?event, proxy = ?proxy, "ZcosmicToplevelInfoV1");
        if let zcosmic_toplevel_info_v1::Event::Toplevel { toplevel } = event {
            app_data.apps.push(App {
                handle: toplevel,
                title: None,
                app_id: None,
                outputs: Vec::new(),
                workspaces: Vec::new(),
                state: Vec::new(),
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
        tracing::debug!(event = ?event, proxy = ?toplevel, "ZcosmicToplevelHandleV1");
        let Some(info) = app_data.apps.iter_mut().find(|t| &t.handle == toplevel) else {
            return;
        };
        match event {
            zcosmic_toplevel_handle_v1::Event::Title { title } => {
                info.title = Some(title);
            }
            zcosmic_toplevel_handle_v1::Event::AppId { app_id } => {
                info.app_id = Some(app_id);
            }
            zcosmic_toplevel_handle_v1::Event::OutputEnter { output } => {
                info.outputs.push(output.id());
            }
            zcosmic_toplevel_handle_v1::Event::OutputLeave { output } => {
                let output_id = output.id();
                info.outputs.retain(|o| o != &output_id);
            }
            zcosmic_toplevel_handle_v1::Event::ExtWorkspaceEnter { workspace } => {
                info.workspaces.push(workspace.id());
            }
            zcosmic_toplevel_handle_v1::Event::ExtWorkspaceLeave { workspace } => {
                let workspace_id = workspace.id();
                info.workspaces.retain(|w| w != &workspace_id);
            }
            // zcosmic_toplevel_handle_v1::Event::WorkspaceEnter { workspace } => {
            //         info.workspaces.push(workspace);
            // }
            // zcosmic_toplevel_handle_v1::Event::WorkspaceLeave { workspace } => {
            //         info.workspaces.retain(|w| w != &workspace);
            // }
            zcosmic_toplevel_handle_v1::Event::State { state } => {
                info.state = state
                    .chunks_exact(4)
                    .map(|chunk| u32::from_ne_bytes(chunk.try_into().unwrap()))
                    .flat_map(|val| State::try_from(val).ok())
                    .collect::<Vec<_>>();
            }
            _ => {}
        }
    }
}

impl Dispatch<zcosmic_workspace_manager_v1::ZcosmicWorkspaceManagerV1, ()> for AppState {
    fn event(
        _state: &mut Self,
        proxy: &zcosmic_workspace_manager_v1::ZcosmicWorkspaceManagerV1,
        event: zcosmic_workspace_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        tracing::debug!(event = ?event, proxy = ?proxy, "ZcosmicWorkspaceManagerV1");
    }

    event_created_child!(
        AppState,
        zcosmic_workspace_manager_v1::ZcosmicWorkspaceManagerV1,
        [
            zcosmic_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (zcosmic_workspace_group_handle_v1::ZcosmicWorkspaceGroupHandleV1, ()),
        ]
    );
}

impl Dispatch<zcosmic_workspace_group_handle_v1::ZcosmicWorkspaceGroupHandleV1, ()> for AppState {
    fn event(
        _state: &mut Self,
        proxy: &zcosmic_workspace_group_handle_v1::ZcosmicWorkspaceGroupHandleV1,
        event: zcosmic_workspace_group_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        tracing::debug!(event = ?event, proxy = ?proxy, "ZcosmicWorkspaceGroupHandleV1");
    }
}

impl Dispatch<zcosmic_workspace_manager_v2::ZcosmicWorkspaceManagerV2, ()> for AppState {
    fn event(
        _state: &mut Self,
        proxy: &zcosmic_workspace_manager_v2::ZcosmicWorkspaceManagerV2,
        event: zcosmic_workspace_manager_v2::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        tracing::debug!(event = ?event, proxy = ?proxy, "ZcosmicWorkspaceManagerV2");
    }
}

impl Dispatch<zcosmic_workspace_handle_v2::ZcosmicWorkspaceHandleV2, ()> for AppState {
    fn event(
        _state: &mut Self,
        proxy: &zcosmic_workspace_handle_v2::ZcosmicWorkspaceHandleV2,
        event: zcosmic_workspace_handle_v2::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        tracing::debug!(event = ?event, proxy = ?proxy, "ZcosmicWorkspaceHandleV2");
    }
}

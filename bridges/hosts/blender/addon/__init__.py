"""Blender Add-on entry point for the bundled Flint Python Bridge."""

import bpy
from bpy.props import BoolProperty, IntProperty, PointerProperty, StringProperty

bl_info = {
    "name": "Flint Bridge",
    "author": "Flint",
    "version": (0, 1, 0),
    "blender": (4, 2, 0),
    "location": "Preferences > Add-ons; 3D View > Sidebar > Flint",
    "description": "Connect Blender to a running Flint backend",
    "category": "Development",
}


class FlintBridgeDraft(bpy.types.PropertyGroup):
    address: StringProperty(name="Bridge address", default="127.0.0.1")
    port: IntProperty(name="Bridge port", default=6321, min=1, max=65535)
    instance_name: StringProperty(name="Instance name", default="Blender")
    enabled: BoolProperty(name="Connect to Flint", default=True)


def _kv_row(layout, label, label_fraction):
    row = layout.split(factor=label_fraction, align=True)
    row.label(text=label)
    return row


def _draw_controls(layout, context):
    from .flint_bridge.blender import manager

    draft = context.window_manager.flint_bridge_draft
    bridge = manager.current()
    snapshot = bridge.status if bridge else None
    label_fraction = 0.42 if context.area.type == "VIEW_3D" else 0.26
    connection = layout.box()
    connection.label(text="Connection")
    _kv_row(connection, "Status", label_fraction).label(
        text=snapshot["connection"]["state"].replace("_", " ").title() if snapshot else "Stopped"
    )
    if snapshot:
        active = snapshot["settings"]
        _kv_row(connection, "Active settings", label_fraction).label(
            text="{}:{} · {}".format(active["address"], active["port"], active["name"])
        )
        obstacle = snapshot["connection"].get("obstacle")
        if obstacle:
            warning = layout.box()
            warning.alert = True
            warning.label(text=obstacle["message"], icon="ERROR")
    else:
        _kv_row(connection, "Active settings", label_fraction).label(text="—")
    settings = layout.box()
    settings.label(text="Settings")
    for label, field in (
        ("Bridge address", "address"),
        ("Bridge port", "port"),
        ("Instance name", "instance_name"),
        ("Connect to Flint", "enabled"),
    ):
        _kv_row(settings, label, label_fraction).prop(draft, field, text="")
    row = layout.row()
    # Without a Bridge, Apply starts one from these settings.
    row.enabled = not (snapshot and snapshot["busy"])
    row.operator("flint_bridge.apply_settings", text="Apply")
    retry = row.row()
    retry.enabled = bool(snapshot and snapshot["settings"]["enabled"])
    retry.operator("flint_bridge.reconnect", text="Reconnect")


class FlintBridgePreferences(bpy.types.AddonPreferences):
    bl_idname = __name__

    address: StringProperty(default="127.0.0.1", options={"HIDDEN"})
    port: IntProperty(default=6321, min=1, max=65535, options={"HIDDEN"})
    instance_name: StringProperty(default="Blender", options={"HIDDEN"})
    enabled: BoolProperty(default=True, options={"HIDDEN"})

    def draw(self, context):
        _draw_controls(self.layout, context)


class FLINT_OT_apply_settings(bpy.types.Operator):
    bl_idname = "flint_bridge.apply_settings"
    bl_label = "Apply Flint Connection Settings"

    def execute(self, context):
        from .flint_bridge.blender import manager

        draft = context.window_manager.flint_bridge_draft
        try:
            manager.configure(
                address=draft.address,
                port=draft.port,
                name=draft.instance_name,
                enabled=draft.enabled,
            )
        except (ValueError, RuntimeError) as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        preferences = context.preferences.addons[__name__].preferences
        for field in ("address", "port", "instance_name", "enabled"):
            setattr(preferences, field, getattr(draft, field))
        return {"FINISHED"}


class FLINT_OT_reconnect(bpy.types.Operator):
    bl_idname = "flint_bridge.reconnect"
    bl_label = "Reconnect Flint Bridge"

    def execute(self, context):
        from .flint_bridge.blender import manager

        try:
            manager.reconnect()
        except RuntimeError as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        return {"FINISHED"}


class FLINT_PT_connection(bpy.types.Panel):
    bl_label = "Flint Bridge"
    bl_idname = "FLINT_PT_connection"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "Flint"

    def draw(self, context):
        _draw_controls(self.layout, context)


_CLASSES = (FlintBridgeDraft, FlintBridgePreferences, FLINT_OT_apply_settings, FLINT_OT_reconnect, FLINT_PT_connection)


def _refresh_ui():
    for window in bpy.context.window_manager.windows:
        for area in window.screen.areas:
            if area.type in {"PROPERTIES", "VIEW_3D"}:
                area.tag_redraw()
    return 1.0


def register():
    from .flint_bridge import BridgeCreationError
    from .flint_bridge.blender import manager

    for klass in _CLASSES:
        bpy.utils.register_class(klass)
    bpy.types.WindowManager.flint_bridge_draft = PointerProperty(type=FlintBridgeDraft)
    preferences = bpy.context.preferences.addons[__name__].preferences
    draft = bpy.context.window_manager.flint_bridge_draft
    for field in ("address", "port", "instance_name", "enabled"):
        setattr(draft, field, getattr(preferences, field))
    try:
        try:
            manager.connect(
                address=preferences.address,
                port=preferences.port,
                name=preferences.instance_name,
                enabled=preferences.enabled,
            )
        except BridgeCreationError as error:
            # The panel stays available so the user can start it with Apply.
            print("Flint Bridge did not start: {}".format(error))
        bpy.app.timers.register(_refresh_ui, first_interval=1.0, persistent=True)
    except BaseException:
        del bpy.types.WindowManager.flint_bridge_draft
        for klass in reversed(_CLASSES):
            bpy.utils.unregister_class(klass)
        raise


def unregister():
    from .flint_bridge.blender import manager

    if not manager.disconnect():
        raise RuntimeError("Flint Bridge is still executing host code")
    if bpy.app.timers.is_registered(_refresh_ui):
        bpy.app.timers.unregister(_refresh_ui)
    del bpy.types.WindowManager.flint_bridge_draft
    for klass in reversed(_CLASSES):
        bpy.utils.unregister_class(klass)

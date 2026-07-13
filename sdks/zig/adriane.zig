const std = @import("std");

pub const AdrianeResult = extern struct {
    code: c_int,
    value: ?[*:0]u8,
    err: ?[*:0]u8,
};

pub const AdrianeStringCallback = ?*const fn (
    payload_json: [*:0]const u8,
    user_data: ?*anyopaque,
    value: *?[*:0]const u8,
    err: *?[*:0]const u8,
) callconv(.C) c_int;
pub const AdrianeEventCallback = ?*const fn (payload_json: [*:0]const u8, user_data: ?*anyopaque) callconv(.C) void;

pub const AdrianeCallbacks = extern struct {
    user_data: ?*anyopaque,
    on_node: AdrianeStringCallback,
    on_condition: AdrianeStringCallback,
    on_event: AdrianeEventCallback,
};

extern fn adriane_engine_version() ?[*:0]u8;
extern fn adriane_validate_graph_json(definition_json: [*:0]const u8) AdrianeResult;
extern fn adriane_compile_graph_yaml_json(yaml: [*:0]const u8) AdrianeResult;
extern fn adriane_available_providers_json() AdrianeResult;
extern fn adriane_resolve_model_json(tier: [*:0]const u8, available_json: ?[*:0]const u8, override_json: ?[*:0]const u8) AdrianeResult;
extern fn adriane_list_components_json() AdrianeResult;
extern fn adriane_list_prebuilt_json() AdrianeResult;
extern fn adriane_run_component_json(kind: [*:0]const u8, params_json: [*:0]const u8, channels_json: [*:0]const u8) AdrianeResult;
extern fn adriane_run_prebuilt_json(name: [*:0]const u8, input_json: [*:0]const u8, options_json: ?[*:0]const u8) AdrianeResult;
extern fn adriane_engine_run_json(spec_json: [*:0]const u8, callbacks: AdrianeCallbacks) AdrianeResult;
extern fn adriane_engine_resume_json(spec_json: [*:0]const u8, callbacks: AdrianeCallbacks) AdrianeResult;
extern fn adriane_engine_approve_and_resume_json(spec_json: [*:0]const u8, callbacks: AdrianeCallbacks) AdrianeResult;
extern fn adriane_engine_signal_json(spec_json: [*:0]const u8, signal_name: [*:0]const u8, payload_json: [*:0]const u8, callbacks: AdrianeCallbacks) AdrianeResult;
extern fn adriane_engine_replay_json(spec_json: [*:0]const u8, checkpoint_id: [*:0]const u8, callbacks: AdrianeCallbacks) AdrianeResult;
extern fn adriane_string_free(ptr: ?[*:0]u8) void;
extern fn adriane_result_free(result: AdrianeResult) void;

pub const Error = error{
    NativeError,
    NullVersion,
};

pub fn engineVersion(allocator: std.mem.Allocator) ![]u8 {
    const ptr = adriane_engine_version() orelse return Error.NullVersion;
    defer adriane_string_free(ptr);
    return allocator.dupe(u8, std.mem.span(ptr));
}

pub fn validateGraphJson(allocator: std.mem.Allocator, definition_json: [:0]const u8) ![]u8 {
    return unwrap(allocator, adriane_validate_graph_json(definition_json.ptr));
}

pub fn compileGraphYamlJson(allocator: std.mem.Allocator, yaml: [:0]const u8) ![]u8 {
    return unwrap(allocator, adriane_compile_graph_yaml_json(yaml.ptr));
}

pub fn availableProvidersJson(allocator: std.mem.Allocator) ![]u8 {
    return unwrap(allocator, adriane_available_providers_json());
}

pub fn resolveModelJson(
    allocator: std.mem.Allocator,
    tier: [:0]const u8,
    available_json: ?[:0]const u8,
    override_json: ?[:0]const u8,
) ![]u8 {
    return unwrap(
        allocator,
        adriane_resolve_model_json(
            tier.ptr,
            if (available_json) |value| value.ptr else null,
            if (override_json) |value| value.ptr else null,
        ),
    );
}

pub fn listComponentsJson(allocator: std.mem.Allocator) ![]u8 {
    return unwrap(allocator, adriane_list_components_json());
}

pub fn listPrebuiltJson(allocator: std.mem.Allocator) ![]u8 {
    return unwrap(allocator, adriane_list_prebuilt_json());
}

pub fn runComponentJson(
    allocator: std.mem.Allocator,
    kind: [:0]const u8,
    params_json: [:0]const u8,
    channels_json: [:0]const u8,
) ![]u8 {
    return unwrap(allocator, adriane_run_component_json(kind.ptr, params_json.ptr, channels_json.ptr));
}

pub fn runPrebuiltJson(
    allocator: std.mem.Allocator,
    name: [:0]const u8,
    input_json: [:0]const u8,
    options_json: ?[:0]const u8,
) ![]u8 {
    return unwrap(
        allocator,
        adriane_run_prebuilt_json(name.ptr, input_json.ptr, if (options_json) |value| value.ptr else null),
    );
}

pub fn engineRunJson(allocator: std.mem.Allocator, spec_json: [:0]const u8, callbacks: AdrianeCallbacks) ![]u8 {
    return unwrap(allocator, adriane_engine_run_json(spec_json.ptr, callbacks));
}

pub fn engineResumeJson(allocator: std.mem.Allocator, spec_json: [:0]const u8, callbacks: AdrianeCallbacks) ![]u8 {
    return unwrap(allocator, adriane_engine_resume_json(spec_json.ptr, callbacks));
}

pub fn engineApproveAndResumeJson(allocator: std.mem.Allocator, spec_json: [:0]const u8, callbacks: AdrianeCallbacks) ![]u8 {
    return unwrap(allocator, adriane_engine_approve_and_resume_json(spec_json.ptr, callbacks));
}

pub fn engineSignalJson(
    allocator: std.mem.Allocator,
    spec_json: [:0]const u8,
    signal_name: [:0]const u8,
    payload_json: [:0]const u8,
    callbacks: AdrianeCallbacks,
) ![]u8 {
    return unwrap(allocator, adriane_engine_signal_json(spec_json.ptr, signal_name.ptr, payload_json.ptr, callbacks));
}

pub fn engineReplayJson(
    allocator: std.mem.Allocator,
    spec_json: [:0]const u8,
    checkpoint_id: [:0]const u8,
    callbacks: AdrianeCallbacks,
) ![]u8 {
    return unwrap(allocator, adriane_engine_replay_json(spec_json.ptr, checkpoint_id.ptr, callbacks));
}

fn unwrap(allocator: std.mem.Allocator, result: AdrianeResult) ![]u8 {
    defer adriane_result_free(result);
    if (result.code == 0) {
        const ptr = result.value orelse return allocator.dupe(u8, "");
        return allocator.dupe(u8, std.mem.span(ptr));
    }

    return Error.NativeError;
}

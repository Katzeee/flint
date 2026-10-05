#ifndef FLINT_BRIDGE_CORE_H
#define FLINT_BRIDGE_CORE_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct FlintBridgeCore FlintBridgeCore;

/* JSON text uses UTF-8. Returned strings must be released with
 * flint_bridge_string_free. A null poll result means no event is available.
 * The caller must stop polling and finish host workers before destroy. */
uint32_t flint_bridge_abi_version(void);
/* error_out may be null. Otherwise it receives null on success or allocated
 * UTF-8 error text on failure; release that text with flint_bridge_string_free. */
FlintBridgeCore *flint_bridge_create(const char *config_json, char **error_out);
char *flint_bridge_poll(const FlintBridgeCore *core, uint32_t timeout_ms);
/* True means the core accepts the report, not that the backend receives it. */
bool flint_bridge_report_execution(const FlintBridgeCore *core, const char *report_json);
bool flint_bridge_connected(const FlintBridgeCore *core);
bool flint_bridge_busy(const FlintBridgeCore *core);
char *flint_bridge_instance_id(const FlintBridgeCore *core);
bool flint_bridge_reconnect(const FlintBridgeCore *core);
char *flint_bridge_status_json(const FlintBridgeCore *core);
/* Returns 0 when applied, 1 while executing, or 2 for invalid settings. */
uint32_t flint_bridge_apply_settings(const FlintBridgeCore *core, const char *settings_json);
void flint_bridge_stop(const FlintBridgeCore *core);
void flint_bridge_destroy(FlintBridgeCore *core);
void flint_bridge_string_free(char *value);

/* Creation options: {"host","address","port","name","runtime_version"}, with
 * optional "enabled" (default true). A host process owns at most one Bridge:
 * create returns null if one already exists. Settings: {"address","port","name","enabled"}.
 * Poll returns an execute event with request_id,
 * workflow_id, execution_id, code, and optional filename. Execution reports are
 * {"kind":"output","request_id","stdout","stderr"} or
 * {"kind":"result","request_id","succeeded","traceback","error"}.
 * At most one execution is active; another request receives instance_busy. */

#ifdef __cplusplus
}
#endif

#endif

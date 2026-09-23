#ifndef FLINT_BRIDGE_CORE_H
#define FLINT_BRIDGE_CORE_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct FlintBridgeCore FlintBridgeCore;

/* All text is UTF-8 JSON. Returned strings belong to the core and must be
 * released with flint_bridge_string_free. A null poll result means timeout.
 * The caller must stop polling and finish host workers before destroy. */
uint32_t flint_bridge_abi_version(void);
FlintBridgeCore *flint_bridge_create(const char *config_json);
char *flint_bridge_poll(const FlintBridgeCore *core, uint32_t timeout_ms);
bool flint_bridge_submit(const FlintBridgeCore *core, const char *command_json);
bool flint_bridge_connected(const FlintBridgeCore *core);
bool flint_bridge_busy(const FlintBridgeCore *core);
char *flint_bridge_instance_id(const FlintBridgeCore *core);
void flint_bridge_reconnect(const FlintBridgeCore *core);
void flint_bridge_stop(const FlintBridgeCore *core);
void flint_bridge_destroy(FlintBridgeCore *core);
void flint_bridge_string_free(char *value);

/* Config: {"host","address","port","name","runtime_version"}.
 * Poll returns an execute event with request_id,
 * workflow_id, execution_id, code, and optional filename. Submit accepts
 * {"kind":"output","request_id","stdout","stderr"} or
 * {"kind":"result","request_id","succeeded","traceback","error"}.
 * At most one execution is active; another request receives instance_busy. */

#ifdef __cplusplus
}
#endif

#endif

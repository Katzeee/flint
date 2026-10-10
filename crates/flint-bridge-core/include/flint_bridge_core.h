#ifndef FLINT_BRIDGE_CORE_H
#define FLINT_BRIDGE_CORE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct FlintBridgeCore FlintBridgeCore;
typedef struct FlintStep FlintStep;

/* The core orchestrates each execution through these host callbacks:
 * post, then prepare and run on the host's execution thread, or discard when
 * the execution stops before running. Callbacks must not unwind.
 *
 * A step identifies one use of an execution the host owes the core; the host
 * never dereferences it. Using a step after it is used, or after destroy, has
 * no effect, so the host may drop one without releasing anything.
 *
 * post is called from a core dispatcher thread, never the network thread. It
 * returns true after arranging exactly one later flint_step_run on the
 * execution thread, and false if it cannot.
 *
 * prepare and run each receive a step to complete exactly once, before or
 * after returning and from any thread. Output is accepted until completion.
 * prepare receives the request JSON {"request_id","workflow_id","execution_id",
 * "code","filename"}, borrowed for the call, and succeeds with an ID for its prepared
 * result. The core passes this ID to run or to discard; discard may run on any thread.
 * A failure without a code is reported as preparation_failed or
 * execution_failed by the step that failed.
 *
 * release is called once, after the last running callback, when the core is
 * destroyed or creation fails. */
typedef struct FlintExecutionBinding {
    uintptr_t context;
    bool (*post)(uintptr_t context, FlintStep *step);
    void (*prepare)(uintptr_t context, const char *request_json, FlintStep *step);
    void (*run)(uintptr_t context, uintptr_t result_id, FlintStep *step);
    void (*discard)(uintptr_t context, uintptr_t result_id);
    void (*release)(uintptr_t context);
} FlintExecutionBinding;

/* JSON text uses UTF-8. Returned strings must be released with
 * flint_bridge_string_free. */
uint32_t flint_bridge_abi_version(void);
typedef enum FlintCreationErrorKind {
    FLINT_CREATION_INVALID_CONFIGURATION = 1,
    FLINT_CREATION_CLAIMED = 2,
    FLINT_CREATION_SYSTEM = 3
} FlintCreationErrorKind;
/* Creation failures report a FlintCreationErrorKind through error_kind.
 * Either output may be null. Free error_message with flint_bridge_string_free. */
FlintBridgeCore *flint_bridge_create(const char *config_json,
                                    const FlintExecutionBinding *execution_binding,
                                    uint32_t *error_kind, char **error_message);
bool flint_bridge_connected(const FlintBridgeCore *core);
bool flint_bridge_busy(const FlintBridgeCore *core);
bool flint_bridge_stopped(const FlintBridgeCore *core);
char *flint_bridge_instance_id(const FlintBridgeCore *core);
bool flint_bridge_reconnect(const FlintBridgeCore *core);
char *flint_bridge_status_json(const FlintBridgeCore *core);
/* Returns 0 when applied, 1 while executing, 2 for invalid settings, 3 after stop.
 * Reconnect likewise returns false after stop. */
uint32_t flint_bridge_apply_settings(const FlintBridgeCore *core, const char *settings_json);
/* Terminal: ends the connection and cancels execution whose code has not
 * started. Returns false while prepare or run is still outstanding. */
bool flint_bridge_stop(const FlintBridgeCore *core);
/* Stops, revokes every callback, and releases the process claim and every step
 * the host still holds. Call it after stop returns true, unless the hosting
 * context itself is ending. */
void flint_bridge_destroy(FlintBridgeCore *core);
void flint_bridge_string_free(char *value);

void flint_step_run(FlintStep *step);
/* Spans are UTF-8; a zero-length span may be null. Returns false once the step
 * no longer accepts output. */
bool flint_step_output(FlintStep *step, const uint8_t *stdout, size_t stdout_len,
                       const uint8_t *stderr, size_t stderr_len);
void flint_step_succeed(FlintStep *step, uintptr_t result_id);
/* Strings are borrowed for the call and may be null. A null or blank code or
 * message uses the failed stage's default. Unknown codes pass through unchanged.
 * message is diagnostic text; traceback carries the host's stack trace. */
void flint_step_fail(FlintStep *step, const char *code, const char *message, const char *traceback);

/* Creation options: {"host","runtime_version","settings"}.
 * A host process owns at most one Bridge:
 * create returns null if one already exists. Settings: {"address","port","name"}.
 * At most one execution is active; another request receives instance_busy. */

#ifdef __cplusplus
}
#endif

#endif

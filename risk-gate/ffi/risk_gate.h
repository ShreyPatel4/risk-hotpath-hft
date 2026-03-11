/* risk-gate C API
 *
 * Zero-allocation pre-trade risk gate.
 * Link with: -lrisk_gate -lpthread -ldl -lm
 *
 * Build: cargo build --release -p risk-gate --features ffi
 */

#ifndef RISK_GATE_H
#define RISK_GATE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* --- Types (repr(C), match Rust layout) --- */

typedef enum {
    SIDE_BUY  = 0,
    SIDE_SELL = 1,
} Side;

typedef enum {
    DECISION_ACCEPT              = 0,
    DECISION_REJECT_MAX_QUANTITY = 1,
    DECISION_REJECT_MAX_NOTIONAL = 2,
    DECISION_REJECT_PRICE_COLLAR = 3,
    DECISION_REJECT_CREDIT_LIMIT = 4,
    DECISION_REJECT_DUPLICATE    = 5,
    DECISION_REJECT_ZERO_QTY     = 6,
    DECISION_REJECT_INVALID_PRICE = 7,
} Decision;

typedef struct {
    uint64_t order_id;
    uint32_t symbol_id;
    uint32_t trader_id;
    double   price;
    uint64_t quantity;
    Side     side;
} Order;

typedef struct {
    uint64_t max_quantity;
    double   max_notional;
    double   credit_limit;
    double   collar_lower;    /* e.g. 0.95 = -5% */
    double   collar_upper;    /* e.g. 1.05 = +5% */
    uint64_t dup_window_ns;
} RiskConfig;

/* Opaque handle */
typedef struct RiskGateHandle RiskGateHandle;

/* --- API --- */

/* Create a new risk gate. Returns heap-allocated handle (the only allocation). */
RiskGateHandle* risk_gate_new(RiskConfig config);

/* Evaluate an order. Returns Decision enum value. */
Decision risk_gate_evaluate(RiskGateHandle* gate, const Order* order,
                            double ref_price, uint64_t now_ns);

/* Hot-swap the configuration. */
void risk_gate_set_config(RiskGateHandle* gate, RiskConfig config);

/* Query trader exposure. */
double risk_gate_trader_exposure(const RiskGateHandle* gate, uint32_t trader_id);

/* Reset all credit exposures. */
void risk_gate_reset_credit(RiskGateHandle* gate);

/* Destroy the gate and free memory. */
void risk_gate_destroy(RiskGateHandle* gate);

#ifdef __cplusplus
}
#endif

#endif /* RISK_GATE_H */

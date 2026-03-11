/*
 * Example: Using risk-gate from C.
 *
 * Build:
 *   cd risk-hotpath-hft
 *   cargo build --release -p risk-gate --features ffi
 *   cd examples/c_integration
 *   make
 */

#include "../../risk-gate/ffi/risk_gate.h"
#include <stdio.h>
#include <time.h>

static const char* decision_name(Decision d) {
    switch (d) {
        case DECISION_ACCEPT:              return "ACCEPT";
        case DECISION_REJECT_MAX_QUANTITY: return "REJECT(max_qty)";
        case DECISION_REJECT_MAX_NOTIONAL: return "REJECT(max_notional)";
        case DECISION_REJECT_PRICE_COLLAR: return "REJECT(price_collar)";
        case DECISION_REJECT_CREDIT_LIMIT: return "REJECT(credit_limit)";
        case DECISION_REJECT_DUPLICATE:    return "REJECT(duplicate)";
        case DECISION_REJECT_ZERO_QTY:     return "REJECT(zero_qty)";
        case DECISION_REJECT_INVALID_PRICE:return "REJECT(invalid_price)";
        default:                           return "UNKNOWN";
    }
}

int main(void) {
    RiskConfig config = {
        .max_quantity  = 10000,
        .max_notional  = 5000000.0,
        .credit_limit  = 10000000.0,
        .collar_lower  = 0.95,
        .collar_upper  = 1.05,
        .dup_window_ns = 1000000000,  /* 1 second */
    };

    RiskGateHandle* gate = risk_gate_new(config);
    printf("risk-gate C integration demo\n\n");

    /* Valid order */
    Order o1 = { .order_id = 1, .symbol_id = 0, .trader_id = 0,
                 .price = 150.0, .quantity = 100, .side = SIDE_BUY };
    Decision d1 = risk_gate_evaluate(gate, &o1, 150.0, 0);
    printf("Order 1 (valid):     %s\n", decision_name(d1));

    /* Exceeds max qty */
    Order o2 = { .order_id = 2, .symbol_id = 0, .trader_id = 1,
                 .price = 150.0, .quantity = 50000, .side = SIDE_BUY };
    Decision d2 = risk_gate_evaluate(gate, &o2, 150.0, 1000);
    printf("Order 2 (big qty):   %s\n", decision_name(d2));

    /* Price outside collar */
    Order o3 = { .order_id = 3, .symbol_id = 0, .trader_id = 2,
                 .price = 200.0, .quantity = 10, .side = SIDE_SELL };
    Decision d3 = risk_gate_evaluate(gate, &o3, 150.0, 2000);
    printf("Order 3 (bad price): %s\n", decision_name(d3));

    /* Duplicate */
    Decision d4 = risk_gate_evaluate(gate, &o1, 150.0, 500);
    printf("Order 4 (dup of 1):  %s\n", decision_name(d4));

    /* Benchmark: 1M evaluations */
    printf("\nBenchmarking 1M evaluations...\n");
    struct timespec start, end;
    clock_gettime(CLOCK_MONOTONIC, &start);

    risk_gate_reset_credit(gate);
    for (uint64_t i = 0; i < 1000000; i++) {
        Order o = { .order_id = i + 100, .symbol_id = (uint32_t)(i % 120),
                    .trader_id = (uint32_t)(i % 50), .price = 150.0,
                    .quantity = 10, .side = (i % 2 == 0) ? SIDE_BUY : SIDE_SELL };
        risk_gate_evaluate(gate, &o, 150.0, i * 1000);
    }

    clock_gettime(CLOCK_MONOTONIC, &end);
    double elapsed = (end.tv_sec - start.tv_sec) + (end.tv_nsec - start.tv_nsec) / 1e9;
    printf("  1,000,000 evals in %.3fs = %.0f evals/sec (%.0f ns/eval)\n",
           elapsed, 1000000.0 / elapsed, elapsed * 1e9 / 1000000.0);

    risk_gate_destroy(gate);
    return 0;
}

## Five Lines review

_A structural-quality lens (Christian Clausen, *Five Lines of Code*). It is not a correctness review: a method can break every rule below and be correct, and a 3-line method can still have a bug._

8 changed method(s) reviewed · 9 finding(s) · 8 Jev request(s)

### Rule 1 · Five lines
- `orders/pricing.py:39` — `quote` has 10 statements (well over; budget 5) _(mechanical; introduced)_
  - Fix: extract the `if` block at line 44 into its own method

### Rule 2 · Call or pass, not both
- `orders/pricing.py:39` — `quote` both orchestrates collaborators and computes on raw values _(Jev 0.98; introduced)_
  - Fix: keep the calls in `quote`; move the inline computation into a method that is handed the values
- `web/cart.ts:14` — `shippingCost` both orchestrates collaborators and computes on raw values _(Jev 0.95; introduced)_
  - Fix: keep the calls in `shippingCost`; move the inline computation into a method that is handed the values
- `web/cart.ts:22` — `drain` both orchestrates collaborators and computes on raw values _(Jev 0.71; worth a look)_
  - Fix: keep the calls in `drain`; move the inline computation into a method that is handed the values

### Rule 3 · If only at the start
- `orders/pricing.py:44` — the `if` at line 44 in `quote` is not the method's first statement _(mechanical; introduced)_
  - Fix: extract the `if` at line 44 and its block into its own method, so it starts that method

### Rule 4 · Never if-else
- `web/cart.ts:15` — `shippingCost` branches with if/else between two pieces of its own domain logic _(Jev 0.60; worth a look)_
  - Fix: introduce one interface with a class per branch, and push each branch's body into its class

### Rule 5 · Never switch
- `orders/pricing.py:53` — the `match` at line 53 has a catch-all arm _(mechanical; introduced)_
  - Fix: list every case explicitly so a new variant fails loudly, or replace with polymorphism

### Rule 6 · Inherit only from interfaces
- `orders/pricing.py:15` — `ExpressPricing` inherits from `BasePricing`, which carries method bodies: that is inheriting implementation _(mechanical; introduced)_
  - Fix: give `ExpressPricing` a `BasePricing` field and delegate to it; share the contract through an interface

### Rule 7 · Pure conditions
- `web/cart.ts:22` — a condition in `drain` has a side effect _(Jev 0.96; introduced)_
  - Fix: hoist the side effect into its own statement before the condition; split the query from the command

### Rule 8 · No single-implementation interfaces
- `orders/pricing.py:6` — interface `TaxPolicy` has 1 non-test implementer(s): orders/pricing.py _(mechanical; introduced)_
  - Fix: delete `TaxPolicy` and use the concrete class directly until a second implementation exists

### Rule 9 · Avoid getters/setters
- `web/cart.ts:10` — `getTotal` is an accessor that invites callers to do the object's work _(Jev 0.68; worth a look)_
  - Fix: move the caller logic that uses `getTotal` into the owning class (push code into data), then remove the accessor

### Rule 10 · No common affixes
- `orders/pricing.py:39` — `start_date` and `end_date` differ only by the affix start/end _(mechanical; introduced)_
  - Fix: introduce a `DateRange`-style type holding both, and move the logic that uses them onto it

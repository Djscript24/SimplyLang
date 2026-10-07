# SimplyLang Memory Model

This document summarizes programmer-visible copying, mutation, closure capture,
equality, and lifetime behavior. It describes language semantics, not required
implementation details.

## Values, sharing, and mutation

SimplyLang has value-like data and identity-bearing objects. Copying follows the
value's category:

| Category | Copy and mutation behavior | Equality |
|---|---|---|
| `Unit`, numbers, booleans, ranges, strings | Values; bindings may be reassigned when declared `mut` | Value equality |
| Arrays, lists, tuples, hashes, trees, matrices | Value-like containers. Copies may share backing data for reading; supported mutation updates unique storage in place or detaches shared storage | Structural equality for the same kind and contents |
| Structs | Copies refer to the same instance; messages may mutate that instance | Instance identity |
| Enums | Copies retain the variant and payload; payload values follow their own category | Declaration, variant, and payload equality |
| Functions and closures | Copies refer to the same callable | Callable identity |
| `CsvStream` | Copies its source-position descriptor | Descriptor fields |

Supported collection mutation requires a mutable binding. It changes that
container value, not other copies: uniquely usable backing storage can be
updated in place; shared storage is detached first. Struct mutation is
different: aliases observe changes to the same instance. Reassigning a binding
replaces only that binding's value.

The available collection mutation operations depend on the kind: indexed writes
support arrays, lists, and hashes; list `add`/`remove` are supported. Tuples,
trees, and matrices do not support these direct mutation operations.

## Bindings, calls, and returns

Bindings are immutable by default. `mut` permits reassignment and supported
collection mutation; it does not deep-copy a value or make referenced objects
private. Function arguments create local parameter bindings: rebinding a
parameter does not rebind the caller's variable. Collection parameters follow
the same copy-on-write behavior, while struct parameters refer to the same
instance.

Returning a value preserves its ordinary value-category behavior after the
function's scope exits. This includes collections, structs, enums, and callable
closures; no move or lifetime annotation is needed.

## Closure capture

- **Nested functions** snapshot the referenced lexical values when created.
  Captured bindings are immutable in the closure. Collection snapshots follow
  copy-on-write; captured structs retain their instance identity.
- **Executable top-level functions** resolve global/module bindings when called,
  so later global reassignment is visible.
- **Imported functions** capture the module values they depend on for that
  import execution. Module evaluation is isolated; parsed programs may be
  cached.

Closures capture values, not scope frames or live binding slots.

## Identity and equality

Struct equality tests instance identity and does not traverse fields. Collection
and enum equality recursively follows the contained values' category rules;
nominal declarations from different modules remain distinct. COW backing-store,
arena, and heap identity are not directly observable through language equality.

## Scope lifetime and reclamation

Scope exit removes local bindings. It does not invalidate a value still
reachable through a return, closure capture, collection, enum, or another
runtime root. Cycles are legal and remain usable while their roots remain alive.

The runtime does not promise immediate reclamation of every unreachable object
entry; tracing garbage collection is not part of the language contract.
Programmers do not manage memory manually.

## Implementation note

The current runtime uses scope-based bindings in generational arenas, shared
runtime heap arenas for escaping functions/structs/enums/source text, and
copy-on-write collection storage. Handles reject stale or cross-arena access.
These mechanisms may change as long as the language behavior above is
preserved.

SimplyLang has no user-visible ownership, borrowing, move, lifetime, reference
counting, or manual memory-management syntax.

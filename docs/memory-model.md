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
| Arrays, lists, hashes | Shared collection identities. Assignment aliases the same mutable contents; mutation through a `mut` binding is visible through every alias | Structural equality for the same kind and contents |
| Tuples, matrices | Immutable value-like containers | Structural equality for the same kind and contents |
| Structs | Copies refer to the same instance; messages may mutate that instance | Instance identity |
| Enums | Copies retain the variant and payload; payload values follow their own category | Declaration, variant, and payload equality |
| Functions and closures | Copies refer to the same callable | Callable identity |
| `CsvStream` | Copies its source-position descriptor | Descriptor fields |

Supported collection mutation requires a mutable binding. `mut` authorizes a
write through that binding; it does not make the collection private. Every
binding that refers to the same Array, List, or Hash observes the write,
including bindings not declared `mut`. Reassigning a binding replaces only
that binding's reference. Struct mutation has the same alias-visible property.

The available collection mutation operations depend on the kind: indexed writes
support arrays, lists, and hashes, including nested collection paths; list
`add`/`remove` are supported. Tuples and matrices do not support these direct
mutation operations.

## Bindings, calls, and returns

Bindings are immutable by default. `mut` permits reassignment and supported
collection mutation; it does not deep-copy a value or make referenced objects
private. Function arguments create local parameter bindings: rebinding a
parameter does not rebind the caller's variable. Array, List, and Hash values
passed to a function retain their shared collection identity.

Array, List, Hash, and Vector function parameters must be marked `ref` at both
declaration and call:

```simply
fn first(ref values as List[Int]) gives Int:
    return values[0]
end

items is list [10, 20]
Sayln first(ref items)
```

This is a read-only temporary borrow of the collection handle; passing `ref`
does not copy collection contents. The argument may be a nested collection
expression such as `ref profile["scores"]`, which borrows that collection for
the duration of the call. A `ref` parameter cannot be declared `mut`, written
through, returned, or stored in another binding. Values read from it may be
copied into a new escaping collection when they are not themselves collections;
fresh built-in collection results are also allowed when their elements do not
carry borrowed collection handles. Nested collection values that still refer
to borrowed storage cannot escape. A nested function may be declared while a
`ref` parameter is in scope, but it cannot capture that parameter. The marker
is required on exactly those call arguments whose parameters use `ref`.
Functions with `ref` parameters cannot be used as first-class values because
their call-site markers must remain statically visible. Other parameters retain
their existing behavior.

Returning a value preserves its ordinary value-category behavior after the
function's scope exits. This includes collections, structs, enums, and callable
closures; no move or lifetime annotation is needed.

## Closure capture

- **Nested functions** snapshot the referenced lexical values when created.
  Captured bindings are immutable in the closure. Captured collections and
  structs retain their identity, so mutable aliases still observe mutations.
- **Executable top-level functions** resolve global/module bindings when called,
  so later global reassignment is visible.
- **Imported functions** capture the module values they depend on for that
  import execution. Module evaluation is isolated; parsed programs may be
  cached.

Closures capture values, not scope frames or live binding slots.

## Identity and equality

Struct equality tests instance identity and does not traverse fields. Collection
and enum equality recursively follows the contained values' category rules;
nominal declarations from different modules remain distinct. Arena and heap
identity are not directly observable through language equality.

## Scope lifetime and reclamation

Scope exit removes local bindings. It does not invalidate a value still
reachable through a return, closure capture, collection, enum, or another
runtime root. Cycles are legal and remain usable while their roots remain alive.

Collections, structs, enums, functions, and source text are retained in
generational arenas owned by the evaluator. Collection cycles are safe because
collection values contain non-owning handles; no recursive reference-counted
ownership cycle is formed. Arena slots are retained until evaluator teardown;
the runtime does not reclaim unreachable collection nodes during evaluation.
Tracing garbage collection is not part of the language contract. Programmers
do not manage memory manually.

## Implementation note

The current runtime uses scope-based bindings and evaluator-owned generational
arenas. Array/List/Hash values are non-owning generational handles into the
runtime heap; arena slots remain allocated until evaluator teardown. Tuples and
matrices retain value-like shared backing storage. Handles reject stale or
cross-arena access. These mechanisms may change as long as the language
behavior above is preserved.

SimplyLang has no user-visible ownership, move, lifetime, reference-counting,
or manual memory-management syntax. Its limited `ref` marker is only for
read-only collection parameters and does not introduce general references.

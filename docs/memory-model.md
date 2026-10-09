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

Array, List, Hash, and Vector function parameters are marked `ref` in the
function declaration. The call site uses an ordinary argument:

```simply
fn first(ref values as List[Int]) gives Int:
    return values[0]
end

items is list [10, 20]
Sayln first(items)
```

This is a temporary shared borrow of the collection handle; passing it does
not copy collection contents. The argument may be a nested collection
expression such as `profile["scores"]`, which borrows that collection for the
duration of the call. Shared borrows may overlap; writes to the same collection
through another alias are rejected while a shared borrow is active.

An exclusive mutable borrow is declared with `mut ref`:

```simply
fn update(mut ref values as List[Int]):
    values[1] -> 99
end

mut numbers is list [10, 20, 30]
update(numbers)
```

The argument must be a mutable variable. Only the active `mut ref` parameter
may mutate that collection; conflicting shared or exclusive borrows are
rejected. A borrow can be forwarded to a nested call through the corresponding
parameter, without copying collection contents. The borrow ends on return or
error, after which ordinary mutation can proceed again. Aliases are checked at
runtime using the collection's validated generational identity, since distinct
variable names can refer to the same collection.

Parameter borrow forms are call-scoped. Local bindings use `alias is ref owner`
for a shared borrow and `mut alias is ref owner` for an exclusive borrow; the
loan ends when the binding's scope exits. Local ref bindings cannot be
redirected, and a collection binding cannot be reassigned while its collection
is borrowed. A reference cannot be returned, stored in another binding,
collection, or object, or captured by a closure. Shared `ref` parameters are
read-only; `mut ref` permits writes through the parameter but does not permit
reassignment of the parameter binding. Values read from a borrow may be copied
into a new escaping collection when they are not themselves collections;
fresh built-in collection results are also allowed when their elements do not
carry borrowed collection handles. Nested collection values that still refer
to borrowed storage cannot escape. A nested function may be declared while a
borrow parameter is in scope, but it cannot capture that parameter. The
parameter declaration determines the borrow; callers use ordinary arguments.
Functions with either borrow parameter cannot be used as first-class values
because their borrow contract must remain statically visible. Other parameters
retain their existing behavior.

Local bindings may borrow a scalar slot or a Struct field using
`alias is ref values[0]` or `mut alias is ref values[0]`. Hash slots use string
keys, and Struct fields use bracket string keys, for example
`profile["stats"]["score"]`; dot access remains Hash-only. These segments may
be combined into nested paths. Each segment is resolved once when the binding
is created, and missing keys or invalid indices fail at that time. Exclusive
assignment through the alias updates the original field or slot, not a
detached nested collection copy. A local borrow may also target a collection
stored directly in a Struct field; collection-valued slots inside collections
remain unsupported.

Borrow locations retain each collection or Struct's generational arena identity
and the remaining Index, Hash-key, or Struct-field path. These are paths to
positions/keys, not stable element identities. Paths overlap when they share an
identity and match through the shorter path. Whole-object and whole-collection
locations therefore overlap all descendants. Distinct concrete indices, keys,
or fields are disjoint; mixed or uncertain segment relationships
conservatively overlap.

Writes through an exclusive element alias follow its recorded path. Owner-side
collection writes are deliberately stricter: while any path in a collection is
borrowed, replacement through the owner and structural changes such as List
insertion/removal are rejected for the collection identity. This avoids
invalidating index-based paths or silently replacing a nested parent. Struct
field replacement checks its field path, allowing a proven-disjoint field to
be changed. Hash key replacement through the owner is likewise blocked while
any path in that Hash is borrowed; there is no in-place Hash-key deletion
operation. Fixed-size Arrays have indexed replacement but no resizing
operation.

Struct message dispatch is conservative for exclusive borrows: if any
exclusively borrowed identity is reachable from the receiver, dispatch is
rejected because the runtime does not know every field the message and its
nested calls may read. Shared borrows permit dispatch; actual message writes
are checked against the field or collection they mutate. Thus disjoint field
reads/writes are permitted for shared field borrows, while an exclusive field
borrow can reject even a message that would only access an unrelated field.
Scope exit and call/error cleanup release runtime borrow records; handles
continue to validate arena generation on access. Scalar reads through a slot
binding remain ordinary scalar values and may escape under existing value
semantics; the reference binding itself may not escape.

Function reference parameters can be forwarded through nested function or
`::` function-receiver calls. Local exclusive references can be reborrowed for
nested paths; the inner loan is removed on scope/call exit, after which the
outer exclusive reference is usable again. Shared references may be forwarded
or shared with other readers but cannot be used to create an exclusive borrow.
Reference returns, captured references, and collection-valued slots inside
collections remain unsupported.

By-value `for item in collection` and `for mut item in collection` preserve
their existing behavior: the loop iterates over a value snapshot, creates no
iteration borrow, and mutation of the source in the loop body remains allowed.
Shared iteration uses `for ref item in named_collection`. The source must be a
named Array, List, Vector, or Hash; arbitrary expressions, ranges, tuples, and
temporaries are not accepted. It keeps a shared borrow for the entire loop and
iterates over a snapshot, not stable element references. Scalar elements are
copied ordinary values; nested collection/Struct values retain their existing
shared identities. To prevent those identities or shifted List positions from
being mutated through another alias, the loan conservatively covers the whole
iterable and every reachable collection/Struct identity. Owner-side
replacement, structural mutation, and conflicting writes through aliases are
rejected until the loop exits.

The iteration binding is read-only and scoped separately for each iteration.
Its loan is released on normal completion, `break`, `return`, and runtime
error. Semantic checks and runtime guards reject nested collection/Struct
values covered by the loan when they would escape through supported storage
or return operations, and the iteration binding cannot be captured by a
closure. Copied scalar values retain ordinary value behavior. Thus a nested
value cannot be saved for use after the loop, but a scalar read can.
Exclusive reference iteration (`for mut ref`) is not supported. Lists still
identify elements by position, not stable element identity, so exclusive
iteration would need stronger location tracking and a defined
structural-mutation policy.

Ordinary match and destructuring patterns bind values by value. A match arm
may bind a named Array, List, Vector, or Hash using a top-level `ref name`
identifier pattern. This is a shared, read-only borrow of the scrutinee and
all reachable nested collection/Struct storage, released at arm exit including
error and return paths. Such bindings cannot escape through reassignment,
storage, return values, or closure capture. Nested reference patterns, aliases
combining `@` with `ref`, and exclusive reference patterns are unsupported.

Borrowing applies to whole Array, List, Hash, or Vector handles in function
calls and local bindings, scalar leaves in nested collection paths, and Struct
fields selected by bracket string keys. A collection stored directly in a
Struct field may also be borrowed. Struct-valued field references, collection-
valued collection slots, reference returns, slices, and reference capture are
not supported.

### Reference coverage

| Area | Status | Current boundary |
| --- | --- | --- |
| Shared/exclusive function borrows and forwarding | IMPLEMENTED | Runtime conflict checks; tested call-scoped cleanup |
| `::` function receivers | IMPLEMENTED | Receiver mode follows the function's `ref`/`mut ref` declaration and supports forwarding |
| Struct message receivers | PARTIAL | Active exclusive borrows of reachable identities block dispatch; writes check their specific locations |
| Whole-collection local references | IMPLEMENTED | Named collections; lexical-scope lifetime |
| Local indexed/keyed/field references | IMPLEMENTED | Scalar leaves through nested collection paths and Struct fields |
| Collection-valued Struct fields | IMPLEMENTED | A collection directly stored in a Struct field can be borrowed |
| Reassignment, scope exit, error cleanup | IMPLEMENTED | Borrowed owners cannot be rebound; active records leave with scope/call |
| Return, storage, or closure capture of references | UNSUPPORTED BY DESIGN | No escaping-reference lifetime model |
| Object-field disjoint reads during exclusive messages | PARTIAL | Struct message reads conservatively conflict with any active exclusive borrow of the receiver |
| Shared `for ref` iteration | IMPLEMENTED | Named Array/List/Vector/Hash; whole reachable storage is read-locked for loop duration |
| Top-level shared match reference binding | IMPLEMENTED | Named Array/List/Vector/Hash; arm-scoped read lock on reachable storage; no escaping |
| Exclusive iteration and nested reference patterns | UNSUPPORTED BY DESIGN | No stable List element identity; binding is limited to a whole named collection |
| COW interaction | BLOCKED | Existing collections share arena identity; changing to COW would alter current mutation behavior |
| Threads, FFI, raw pointers, smart pointers | UNSUPPORTED BY DESIGN | These are not part of the current runtime model |

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
or manual memory-management syntax. `ref` and `mut ref` only express
call-scoped collection borrows; they do not introduce general references.

# SimplyLang Reference and Borrowing

This document records behavior verified in the current interpreter. The system
uses the existing shared collection handles and runtime borrow records; it does
not introduce ownership syntax, COW, raw pointers, or a general lifetime type
system. See [memory-model.md](memory-model.md) for the broader memory contract.

## Verified semantics

- Function declarations specify parameter mode: `ref values as List[Int]`
  borrows read-only, while `mut ref values as List[Int]` borrows exclusively.
  Call sites use ordinary arguments. Compatible shared borrows may overlap;
  conflicting accesses are rejected at runtime.
- Function borrows can be forwarded through nested function calls. Their
  runtime records are removed when the call returns or propagates an error.
- Local whole-collection bindings use `alias is ref items` or
  `mut alias is ref items`. They borrow named Array, List, Hash, or Vector
  handles through the binding scope; exclusive mode requires a mutable owner.
- A local scalar or collection binding may borrow a path using
  `alias is ref items[index]` or `mut alias is ref items[index]`. Array/List
  integer indices, Hash string keys, and Struct fields written with bracket
  syntax (for example, `profile["stats"]["score"]`) may be combined. Dot syntax
  remains Hash-only. Each segment is evaluated once when the binding is
  created; missing keys and invalid indices fail at that point.
  `alias -> value` through an exclusive element binding writes to the original
  slot, including through nested Struct fields and collections. Runtime borrow
  records retain each generational collection or Struct identity and the
  remaining path; writes traverse the original handles rather than a copied
  collection value.
- Locations overlap only when they share an arena identity and their path
  segments match through the shorter path. A whole-object/collection location
  and a parent path therefore overlap all descendants. Distinct concrete
  indices, Hash keys, or Struct fields are disjoint; mixed or uncertain segment
  kinds conservatively overlap. An exclusive alias can write through its
  recorded path. Writes through the owner collection are more conservative:
  any active borrow of that collection identity blocks owner-side replacement
  or structural mutation, including a replacement at a distinct index/key.
  This prevents list index shifts or replacement of a parent value from
  invalidating a path-based reference. Struct-field replacement checks the
  actual field path, so proven-disjoint fields may be changed independently.
- Owner reassignment is rejected while its collection or Struct location is
  borrowed. Reference
  aliases cannot be rebound. Scope and call exits clean up their respective
  borrow records, including error propagation.
- Collection values continue to use shared generational arena handles.
  Copying a handle does not copy collection contents, and mutation remains
  visible through ordinary aliases. Collection storage is not COW.
- Closures preserve their existing snapshot-by-value capture behavior.
  Borrowed bindings cannot be captured; references cannot be returned or
  stored in other bindings, collections, or objects.
- Existing `::` dispatch is preserved. When dispatch targets a function,
  receiver mode follows its declared `ref`/`mut ref` parameter, including
  forwarding and call/error cleanup. Struct messages check reachable collection
  identities against active exclusive borrows. The check is intentionally
  conservative: any active exclusive borrow whose identity is reachable from
  the Struct receiver blocks dispatch, even if the message might use another
  field. Shared borrows do not block dispatch; message writes still check the
  specific Struct field or collection identity they mutate, so writes to
  proven-disjoint fields remain allowed. Message reads are not field-sensitive.
- Existing reference parameters may be forwarded through nested calls.
  Exclusive local references may be reborrowed for a nested path, and inner
  call/scope cleanup restores use of the outer exclusive reference. A shared
  reborrow cannot be used to mutate while it is active. Failed reborrow
  resolution registers no borrow.
- `for item in collection` and `for mut item in collection` retain their
  by-value behavior and do not borrow the source. `for ref item in
  named_collection` is a shared, read-only iteration form; its source must be
  an identifier bound to an Array, List, Vector, or Hash. Elements are
  snapshotted for iteration: scalars are copied values, while nested
  collection/Struct values retain their existing shared identity. The runtime
  conservatively locks the whole iterable and every reachable nested
  collection/Struct identity for the complete loop, so owner-side replacement,
  structural mutation, and conflicting writes through aliases are rejected.
  Nested collection/Struct identities covered by that loan cannot escape
  through supported storage or return operations; scalar copies can. Static
  checks and runtime guards enforce the no-escape rule. The loan is released on
  normal completion, break, return, and runtime error; the read-only binding is
  scoped separately for each iteration and cannot be captured by a closure.
  `for mut ref` and exclusive reference iteration are unsupported.
- Ordinary match and destructuring patterns bind values. A match arm may use a
  top-level `ref name` identifier pattern only when matching a named Array,
  List, Vector, or Hash. It creates a shared, read-only binding and locks all
  reachable collection/Struct storage for the arm. The binding cannot be
  reassigned, mutated, stored, returned, or captured by a closure. Runtime
  borrow records are released on normal arm completion and on errors/returns.
  Reference patterns inside aliases, OR-patterns, or nested patterns, and
  exclusive reference patterns, remain unsupported.

## Coverage matrix

| Feature/context | Status | Verified boundary or dependency |
| --- | --- | --- |
| Shared function borrows | IMPLEMENTED | Call-scoped collection parameter; runtime conflicts tested |
| Exclusive function borrows | IMPLEMENTED | Mutable named owner required; mutation and conflicts tested |
| Borrow forwarding | IMPLEMENTED | Nested function calls, including exclusive forwarding |
| Local whole-collection binding | IMPLEMENTED | Named Array/List/Hash/Vector; binding-scope lifetime |
| Local scalar/object-field binding | IMPLEMENTED | Scalar leaf through nested Struct fields and Array/List/Hash paths |
| Local collection-valued Struct-field binding | IMPLEMENTED | Whole field collection can be borrowed; collection-valued collection slots remain unsupported |
| Multiple shared readers | IMPLEMENTED | Same collection can be shared during overlapping calls/scopes |
| Disjoint element borrows | IMPLEMENTED | Concrete distinct indices/keys may be borrowed and mutated through their aliases; owner-side collection writes remain conservative |
| Dynamic index aliasing | IMPLEMENTED | Indices/keys are evaluated once and compared at runtime |
| Whole-collection/element conflict | IMPLEMENTED | Runtime checks use the generational collection identity |
| Owner/reference reassignment | IMPLEMENTED | Borrowed owner cannot be rebound; reference alias cannot be redirected |
| Scope/call/error cleanup | IMPLEMENTED | Scope-pop and call checkpoints release active records |
| Shared reference iteration | IMPLEMENTED | Named Array/List/Vector/Hash; locks iterable and reachable nested storage for loop duration |
| Generational arena integration | IMPLEMENTED | Collection and Struct locations carry arena/slot/generation identities |
| Shared alias mutation visibility | IMPLEMENTED | Existing arena-backed collection semantics are preserved |
| Closure capture by value | IMPLEMENTED | Snapshot semantics preserved; capture of active ref binding rejected |
| `::` dispatch | PARTIAL | Function receiver modes are enforced; Struct messages conservatively reject any reachable exclusive borrow and check concrete writes |
| Hash entry borrow | IMPLEMENTED | String-keyed scalar leaf and collection paths |
| Struct field borrow | IMPLEMENTED | Bracket string-key access; no dot syntax |
| Nested collection paths | IMPLEMENTED | Array/List indices, Hash keys, Struct fields; scalar leaves and Struct-field collections |
| Tuple/enum payload references | MISSING | Value/composite storage has no tracked mutable location abstraction |
| Object field references | IMPLEMENTED | Bracket string-key paths anchored to a Struct instance's generational identity |
| Exclusive reference iteration | UNSUPPORTED_BY_DESIGN | No stable element identities; structural mutation remains prohibited during shared iteration |
| Reference pattern binding | UNSUPPORTED_BY_DESIGN | Patterns bind values; no nested location tracking or reference-pattern syntax |
| Return references/reference-bearing types | UNSUPPORTED_BY_DESIGN | No lifetime relationship or escape analysis |
| Closure capture by reference | UNSUPPORTED_BY_DESIGN | Would conflict with snapshot semantics without escape/lifetime checks |
| Temporary borrowing | UNSUPPORTED_BY_DESIGN | Only named collection owners are borrowable |
| Slices/ranges | UNSUPPORTED_BY_DESIGN | No borrowed slice value abstraction |
| COW integration | BLOCKED | Runtime collections intentionally share identity and mutate in place |
| Threads/FFI/raw pointers/smart pointers | UNSUPPORTED_BY_DESIGN | Not provided by the current runtime |

## Deferred work

Field access through a dynamic Struct key has runtime-only type resolution.
Struct message reads are not field-sensitive: any active exclusive borrow
whose identity is reachable from the receiver blocks the message, even when
that borrow names a disjoint field. Shared message access is allowed, with
field/collection write checks performed when the mutation occurs. List indices
identify positions in a collection path, not stable element identities;
structural operations and owner-side slot replacements are therefore blocked
for the borrowed collection identity. Return
references, stored references, and closure capture by reference require
explicit lifetime and escape analysis; retaining a valid arena handle alone is
insufficient.

No performance benchmark is claimed for borrow operations. Current
implementation avoids copying collection contents for whole-collection
borrowing; element bindings copy only the supported scalar value while
retaining runtime location metadata. Allocation counts have not been measured.
/**
 * Ambient override for the koffi module's types.
 *
 * koffi ships its own TypeScript declarations, but its `TypeObject` surface
 * (struct field typing) does not match how this binding drives it — the
 * `koffi.struct(...)` object-literal fields are string C-type names or other
 * struct types, which koffi's own types reject. This module declaration
 * replaces them with the permissive surface this package actually uses
 * (`struct` / `load` / `decode` + a callable `LibraryHandle`). The result
 * stays type-safe at the call sites via the explicit signatures in the
 * `container` / `pipeline` / `device` exports.
 */
declare module "koffi" {
  /**
   * A C type. At runtime koffi struct values are plain JS objects populated by
   * the FFI call — typing them as indexable lets callers read `raw.field`
   * without inventing a parallel mirror of every struct layout.
   */
  export interface TypeObject {
    [field: string]: any;
  }

  /** A bound C function: callable with the declared ABI arguments. */
  export type Callable = (...args: unknown[]) => unknown;

  export interface LibraryHandle {
    /** Declare a C function by signature string, e.g. `"void *mediaway_muxer_create()"`. */
    func(signature: string): Callable;
  }

  /** Declare a C struct layout; field values are C type names or other struct types. */
  export function struct(name: string, def: Record<string, unknown>): TypeObject;

  /** Load a native library (DLL) by path or name. */
  export function load(path: string): LibraryHandle;

  /** Decode bytes at a pointer into a JS value of the given C type. */
  export function decode(ptr: unknown, type: string | TypeObject, length?: number): Uint8Array;

  /** Default export mirrors koffi's CommonJS module.exports. */
  const koffi: {
    struct: typeof struct;
    load: typeof load;
    decode: typeof decode;
  };

  export default koffi;
}

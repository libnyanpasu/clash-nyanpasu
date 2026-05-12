import type {
  LocalProfile_Serialize,
  LocalProfileBuilder_Serialize,
  MergeProfile,
  Profile_Serialize,
  ProfileBuilder_Deserialize,
  ProfileSharedBuilder,
  RemoteProfile_Serialize,
  RemoteProfileBuilder_Serialize,
  ScriptProfile,
  ScriptType,
} from './bindings'

/** Flat profile type for UI consumption — mirrors the pre-rc.25 shape. */
export type NormalizedProfile =
  | ({ type: 'remote' } & RemoteProfile_Serialize)
  | ({ type: 'local' } & LocalProfile_Serialize)
  | ({ type: 'merge' } & MergeProfile)
  | ({ type: 'script' } & ScriptProfile)

/**
 * Flat builder type matching the form-created objects used by the UI.
 * Mirrors the pre-rc.25 ProfileBuilder shape (discriminant `type` at top level).
 */
export type NormalizedProfileBuilder =
  | ({ type: 'remote' } & RemoteProfileBuilder_Serialize)
  | ({ type: 'local' } & LocalProfileBuilder_Serialize)
  | ({ type: 'merge' } & ProfileSharedBuilder)
  | ({ type: 'script'; script_type: ScriptType | null } & ProfileSharedBuilder)

/**
 * Converts the nested specta rc.25 Profile_Serialize into the flat NormalizedProfile
 * the UI code expects.
 */
export function normalizeProfile(item: Profile_Serialize): NormalizedProfile {
  // Exactly one of remote/local/merge/script is defined in the discriminated union.
  return (item.remote ??
    item.local ??
    item.merge ??
    item.script)! as NormalizedProfile
}

/**
 * Converts a flat NormalizedProfileBuilder back into the nested ProfileBuilder_Deserialize
 * required by the backend IPC commands.
 */
export function denormalizeProfileBuilder(
  b: NormalizedProfileBuilder,
): ProfileBuilder_Deserialize {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const raw = b as any
  const { type, ...rest } = raw
  switch (type as string) {
    case 'remote':
      return {
        remote: { type: 'remote', ...rest },
      } as ProfileBuilder_Deserialize
    case 'local':
      return { local: { type: 'local', ...rest } } as ProfileBuilder_Deserialize
    case 'merge':
      return { merge: { type: 'merge', ...rest } } as ProfileBuilder_Deserialize
    case 'script':
      return {
        script: { type: 'script', ...rest },
      } as ProfileBuilder_Deserialize
    default:
      throw new Error(`Unknown profile type: ${type as string}`)
  }
}

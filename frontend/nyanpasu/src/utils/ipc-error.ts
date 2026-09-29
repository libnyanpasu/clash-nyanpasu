import { m } from '@/paraglide/messages'
import type {
  CommitAborted,
  IpcError,
  ProfileContentError,
  ProfileFileError,
  ProfilesError,
  RuntimeAftermath,
} from '@nyanpasu/interface'

/** The simplest message for a failed command, localized by its domain. */
export function ipcErrorMessage(error: IpcError): string {
  switch (error.kind.domain) {
    case 'unknown':
      return error.message
    case 'profiles':
      return profilesErrorMessage(error.kind.error)
  }
}

/**
 * Why a source's commit did not go through, with what became of the running
 * core. Shared by every domain that wraps `CommitAborted`.
 */
export function commitAbortedMessage(error: CommitAborted): string {
  const message = (() => {
    switch (error.kind) {
      case 'write_config':
        return m.error_commit_write_config()
      case 'recover_after_write_failure':
        return m.error_commit_recover_after_write_failure()
      case 'runtime_refused':
        return m.error_commit_runtime_refused({
          reason: error.reasons[0] ?? '',
        })
      case 'runtime_failed':
        return m.error_commit_runtime_failed({
          reason: error.reasons[0] ?? '',
        })
      case 'validate_state':
        return m.error_commit_validate_state()
    }
  })()
  const aftermath =
    error.kind === 'validate_state'
      ? undefined
      : runtimeAftermathMessage(error.runtime)
  return aftermath ? `${message} (${aftermath})` : message
}

function runtimeAftermathMessage(runtime: RuntimeAftermath) {
  switch (runtime.kind) {
    case 'untouched':
      return undefined
    case 'rolled_back':
      return m.error_commit_runtime_rolled_back()
    case 'rollback_failed':
    case 'unknown':
      return m.error_commit_runtime_unknown()
  }
}

/** A subscription URL can carry a token, so only its host is shown. */
function hostOf(url: string) {
  try {
    return new URL(url).host
  } catch {
    return ''
  }
}

function profilesErrorMessage(error: ProfilesError): string {
  switch (error.kind) {
    case 'profile_not_found':
      return m.error_profiles_profile_not_found()
    case 'profile_has_no_file':
      return m.error_profiles_profile_has_no_file()
    case 'not_a_remote_profile':
      return m.error_profiles_not_a_remote_profile()
    case 'profile_file_not_writable':
      return m.error_profiles_profile_file_not_writable()
    case 'profile_in_use':
      return error.referrers.length > 0
        ? m.error_profiles_profile_in_use_by_profiles({
            count: error.referrers.length,
          })
        : m.error_profiles_profile_in_use_by_selection()
    case 'profile_id_collision':
      return m.error_profiles_profile_id_collision()
    case 'validation_failed':
      return m.error_profiles_validation_failed()
    case 'reorder_list_size_mismatch':
    case 'reorder_list_duplicate':
      return m.error_profiles_reorder_invalid()
    case 'revision_overflow':
      return m.error_profiles_revision_overflow()
    case 'invalid_subscription_url':
      return m.error_profiles_invalid_subscription_url()
    case 'remote_profile_needs_import':
      return m.error_profiles_remote_profile_needs_import()
    case 'refresh_in_progress':
      return m.error_profiles_refresh_in_progress()
    case 'fetch_subscription': {
      const host = hostOf(error.url)
      return error.source.kind === 'subscription_http_status'
        ? m.error_profiles_fetch_subscription_status({
            host,
            status: error.source.status,
          })
        : m.error_profiles_fetch_subscription_failed({ host })
    }
    case 'profile_content_rejected':
      return profileContentMessage(error.source)
    case 'profile_file_not_yaml':
      return m.error_profiles_profile_file_not_yaml()
    case 'profile_deleted_during_refresh':
      return m.error_profiles_profile_deleted_during_refresh()
    case 'profile_changed_during_refresh':
      return m.error_profiles_profile_changed_during_refresh()
    case 'fingerprint_definition':
      return m.error_profiles_fingerprint_definition()
    case 'read_profile_file':
      return profileFileMessage(
        error.source,
        m.error_profiles_read_profile_file(),
      )
    case 'profile_file_missing':
      return m.error_profiles_profile_file_missing()
    case 'read_external_profile':
      return profileFileMessage(
        error.source,
        m.error_profiles_read_external_profile({ path: error.target }),
      )
    case 'materialization':
      return profileFileMessage(error.source, m.error_profiles_storage_failed())
    case 'version_conflict':
      return m.error_profiles_version_conflict()
    case 'commit':
      return commitAbortedMessage(error.source)
    case 'workflow_not_ready':
      return m.error_profiles_workflow_not_ready()
    case 'profiles_actor_stopped':
    case 'profiles_reply_dropped':
    case 'blocking_task_cancelled':
      return m.error_profiles_service_stopped()
    case 'shutting_down':
      return m.error_profiles_shutting_down()
  }
}

function profileContentMessage(error: ProfileContentError): string {
  switch (error.kind) {
    case 'not_yaml_mapping':
    case 'reserialize_yaml':
      return m.error_profiles_content_not_yaml()
    case 'missing_proxies':
      return m.error_profiles_content_missing_proxies()
    case 'empty_script':
      return m.error_profiles_content_empty_script()
  }
}

/** Only a path the app refuses is worth telling apart; the rest is the fallback. */
function profileFileMessage(error: ProfileFileError, fallback: string): string {
  switch (error.kind) {
    case 'reserved_path':
    case 'path_escapes_profiles_dir':
    case 'unexpected_node':
    case 'unexpected_symlink':
    case 'existing_file_blocks_symlink':
    case 'symlink_target_not_utf8':
      return m.error_profiles_storage_unsafe_path()
    default:
      return fallback
  }
}

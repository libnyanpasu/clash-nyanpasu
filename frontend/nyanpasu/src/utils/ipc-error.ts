import { m } from '@/paraglide/messages'
import type {
  CommitAborted,
  CoreErrorKind,
  CoreFailure,
  InstallCoreBinaryError,
  IpcError,
  PickPortError,
  PortField,
  ProfileContentError,
  ProfileFileError,
  ProfilesError,
  RuntimeAftermath,
  RuntimeBuildError,
  RuntimeError,
  RuntimePipelineError,
} from '@nyanpasu/interface'

/** The simplest message for a failed command, localized by its domain. */
export function ipcErrorMessage(error: IpcError): string {
  switch (error.kind.domain) {
    case 'unknown':
      return error.message
    case 'profiles':
      return profilesErrorMessage(error.kind.error)
    case 'runtime':
      return runtimeErrorMessage(error.kind.error)
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

function runtimeErrorMessage(error: RuntimeError): string {
  switch (error.kind) {
    case 'shutting_down':
      return m.error_runtime_shutting_down()
    case 'isolated':
      return m.error_runtime_isolated()
    case 'owner_unresponsive':
      return m.error_runtime_owner_unresponsive()
    case 'apply_runtime':
      return coreOperationMessage(
        m.error_runtime_apply_runtime(),
        error.failure,
      )
    case 'stop_core':
      return coreOperationMessage(m.error_runtime_stop_core(), error.failure)
    case 'recover_runtime':
      return coreOperationMessage(
        m.error_runtime_recover_runtime(),
        error.failure,
      )
    case 'recover_service_endpoint':
      return coreOperationMessage(
        m.error_runtime_recover_service_endpoint(),
        error.failure,
      )
    case 'refresh_status':
      return coreOperationMessage(
        m.error_runtime_refresh_status(),
        error.failure,
      )
    case 'install_service':
      return coreOperationMessage(
        m.error_runtime_install_service(),
        error.failure,
      )
    case 'start_service':
      return coreOperationMessage(
        m.error_runtime_start_service(),
        error.failure,
      )
    case 'stop_service':
      return coreOperationMessage(m.error_runtime_stop_service(), error.failure)
    case 'restart_service':
      return coreOperationMessage(
        m.error_runtime_restart_service(),
        error.failure,
      )
    case 'uninstall_service':
      return coreOperationMessage(
        m.error_runtime_uninstall_service(),
        error.failure,
      )
    case 'service_hosts_core':
      return m.error_runtime_service_hosts_core()
    case 'core_not_started':
      return m.error_runtime_core_not_started()
    case 'recovery_unresolved':
      return m.error_runtime_recovery_unresolved()
    case 'build_runtime':
      return buildRuntimeMessage(error.source)
    case 'publish_runtime':
      return m.error_runtime_publish_runtime({ path: error.source.path })
    case 'resolve_port':
      return portMessage(error.source.field, error.source.source)
    case 'resolve_core_binary':
      return error.source.kind === 'find_core_binary'
        ? m.error_runtime_find_core_binary({ core: error.source.core })
        : m.error_runtime_path_not_utf8({ path: error.source.path })
    case 'install_core_binary':
      return installCoreBinaryMessage(error.source)
    case 'prepare_service_install_prompt':
      return m.error_runtime_prepare_service_install_prompt()
    case 'read_core_version':
      return m.error_runtime_read_core_version({ core: error.source.core })
    case 'no_runtime_config':
      return m.error_runtime_no_runtime_config()
    case 'serialize_runtime_config':
    case 'convert_runtime_config':
      return m.error_runtime_render_runtime_config()
    case 'runtime_snapshot_changed':
      return m.error_runtime_runtime_snapshot_changed()
    case 'runtime_node_not_found':
      return m.error_runtime_runtime_node_not_found()
  }
}

/** What the core operation was, and why the core refused it when it said. */
function coreOperationMessage(action: string, failure: CoreFailure): string {
  const reason = failure.kind ? coreReasonMessage(failure.kind) : undefined
  return reason ? `${action} (${reason})` : action
}

function coreReasonMessage(kind: CoreErrorKind): string {
  switch (kind) {
    case 'not_started':
      return m.error_runtime_core_reason_not_started()
    case 'already_running':
      return m.error_runtime_core_reason_already_running()
    case 'revision_conflict':
      return m.error_runtime_core_reason_revision_conflict()
    case 'quarantined':
      return m.error_runtime_core_reason_quarantined()
    case 'config_check_failed':
      return m.error_runtime_core_reason_config_check_failed()
    case 'config_not_found':
      return m.error_runtime_core_reason_config_not_found()
    case 'binary_not_found':
      return m.error_runtime_core_reason_binary_not_found()
    case 'invalid_config':
      return m.error_runtime_core_reason_invalid_config()
    case 'controller_missing':
      return m.error_runtime_core_reason_controller_missing()
    case 'apply_failed':
      return m.error_runtime_core_reason_apply_failed()
    case 'apply_rollback_failed':
      return m.error_runtime_core_reason_apply_rollback_failed()
    case 'stop_unconfirmed':
      return m.error_runtime_core_reason_stop_unconfirmed()
    case 'shutting_down':
      return m.error_runtime_core_reason_shutting_down()
    case 'queue_full':
      return m.error_runtime_core_reason_queue_full()
    case 'operation_conflict':
      return m.error_runtime_core_reason_operation_conflict()
    case 'backend_unavailable':
      return m.error_runtime_core_reason_backend_unavailable()
    case 'internal':
      return m.error_runtime_core_reason_internal()
  }
}

function buildRuntimeMessage(error: RuntimeBuildError): string {
  switch (error.kind) {
    case 'start_script_runner':
      return m.error_runtime_build_start_script_runner()
    case 'validate_profiles':
      return m.error_runtime_build_validate_profiles()
    case 'run_pipeline':
      return pipelineMessage(error.source)
    case 'transforms_failed':
      return m.error_runtime_build_transforms_failed({
        names: error.failures
          .map((failure) =>
            failure.kind === 'profile' ? failure.id : failure.name,
          )
          .join(', '),
      })
    case 'serialize_final_config':
    case 'config_not_mapping':
    case 'serialize_runtime_config':
      return m.error_runtime_render_runtime_config()
  }
}

function pipelineMessage(error: RuntimePipelineError): string {
  switch (error.kind) {
    case 'selected_profile_not_found':
      return m.error_runtime_build_selected_profile_not_found({
        profile: error.profile,
      })
    case 'selected_profile_not_config':
      return m.error_runtime_build_selected_profile_not_config({
        profile: error.profile,
      })
    case 'composition_member_invalid':
      return m.error_runtime_build_composition_member_invalid({
        composition: error.composition,
      })
    case 'content_source':
      return m.error_runtime_build_content_source({ profile: error.profile })
    case 'parse_profile':
      return m.error_runtime_build_parse_profile({ profile: error.profile })
    case 'snapshot':
    case 'internal':
      return m.error_runtime_build_runtime()
  }
}

function portMessage(field: PortField, error: PickPortError): string {
  switch (error.kind) {
    case 'port_not_available':
      return m.error_runtime_port_in_use({ port: error.port })
    case 'no_open_port':
      return m.error_runtime_no_open_port({ field: portFieldName(field) })
  }
}

function portFieldName(field: PortField): string {
  switch (field) {
    case 'mixed':
      return m.error_runtime_port_field_mixed()
    case 'http':
      return m.error_runtime_port_field_http()
    case 'socks':
      return m.error_runtime_port_field_socks()
    case 'external_controller':
      return m.error_runtime_port_field_external_controller()
  }
}

function installCoreBinaryMessage(error: InstallCoreBinaryError): string {
  switch (error.kind) {
    case 'start_elevated_copy':
      return m.error_runtime_install_core_binary_elevation({
        core: error.core,
      })
    case 'elevated_copy_failed':
      return m.error_runtime_install_core_binary_copy({ core: error.core })
    case 'path_not_utf8':
      return m.error_runtime_path_not_utf8({ path: error.path })
  }
}

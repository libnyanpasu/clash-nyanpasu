//! The events, HTTP routes and streams a caller is admitted to, declared for the resource table.
//!
//! Each names its requirement and sensitive marking from the reviewed mapping, and the exposure
//! and capability it had before it became a resource. Each one moves into its namespace when the
//! namespace tree derives requirements.

nyanpasu_rpc::resource!(event "app-update-state-changed", targets = [desktop, http, local], permission = system:read, sensitive, capabilities(app_update));
nyanpasu_rpc::resource!(event "app-update-summary-changed", targets = [desktop, http], permission = system:read, capabilities(app_update));
nyanpasu_rpc::resource!(event "clash-ws-event", targets = [desktop, http, local], permission = traffic:read);
nyanpasu_rpc::resource!(event "configuration-status-changed", targets = [desktop, http, local], permission = system:read, sensitive);
nyanpasu_rpc::resource!(event "configuration-status-summary-changed", targets = [desktop, http], permission = system:read);
nyanpasu_rpc::resource!(event "core-logs-changed", targets = [desktop, http, local], permission = logs:read, sensitive);
nyanpasu_rpc::resource!(event "core-logs-summary-changed", targets = [desktop, http], permission = logs:read);
nyanpasu_rpc::resource!(event "core-status-changed-event", targets = [desktop, http, local], permission = system:read, sensitive);
nyanpasu_rpc::resource!(event "core-status-summary-changed-event", targets = [desktop, http], permission = system:read);
// An empty deep-link notice; it never carries the link.
nyanpasu_rpc::resource!(event "scheme-request-received-event", targets = [desktop], permission = system:read, capabilities(deep_link));
nyanpasu_rpc::resource!(event "service-status-changed-event", targets = [desktop, http, local], permission = system:read, sensitive);
nyanpasu_rpc::resource!(event "service-status-summary-changed-event", targets = [desktop, http], permission = system:read);
nyanpasu_rpc::resource!(event "state-changed-event", targets = [desktop, http], permission = settings:read);
nyanpasu_rpc::resource!(event "storage-resync-event", targets = [desktop, http], permission = settings:read);
// A storage change carries an arbitrary stored value.
nyanpasu_rpc::resource!(event "storage-value-changed-event", targets = [desktop, http], permission = settings:read, sensitive);
// The sync status carries blocked item paths, which are local names; sync is a device-local
// capability that a browser must not read.
nyanpasu_rpc::resource!(event "sync-status-changed-event", targets = [desktop], permission = settings:read, sensitive);
// An inter-window message carries an arbitrary payload and belongs to the desktop shell.
nyanpasu_rpc::resource!(event "window-message-event", targets = [desktop], permission = system:read, sensitive, capabilities(window));

nyanpasu_rpc::resource!(route "icon_fetch", targets = [http], permission = system:read);
nyanpasu_rpc::resource!(route "logs_archive", targets = [http], permission = logs:read, sensitive);
nyanpasu_rpc::resource!(stream "connection_details", targets = [http, local], permission = traffic:read);

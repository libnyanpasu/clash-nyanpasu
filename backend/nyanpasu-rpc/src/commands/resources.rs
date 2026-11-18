//! The events, HTTP routes and streams a caller is admitted to, declared for the resource table.
//!
//! They keep the scope, exposure and capability they had before they became resources. Each one
//! moves into its namespace when the namespace tree derives requirements.

nyanpasu_rpc::resource!(event "app-update-state-changed", targets = [desktop, http, local], scope = sensitive_read, capabilities(app_update));
nyanpasu_rpc::resource!(event "app-update-summary-changed", targets = [desktop, http], scope = read, capabilities(app_update));
nyanpasu_rpc::resource!(event "clash-ws-event", targets = [desktop, http, local], scope = read);
nyanpasu_rpc::resource!(event "configuration-status-changed", targets = [desktop, http, local], scope = sensitive_read);
nyanpasu_rpc::resource!(event "configuration-status-summary-changed", targets = [desktop, http], scope = read);
nyanpasu_rpc::resource!(event "core-logs-changed", targets = [desktop, http, local], scope = sensitive_read);
nyanpasu_rpc::resource!(event "core-logs-summary-changed", targets = [desktop, http], scope = read);
nyanpasu_rpc::resource!(event "core-status-changed-event", targets = [desktop, http, local], scope = sensitive_read);
nyanpasu_rpc::resource!(event "core-status-summary-changed-event", targets = [desktop, http], scope = read);
// An empty deep-link notice; it never carries the link.
nyanpasu_rpc::resource!(event "scheme-request-received-event", targets = [desktop], scope = read, capabilities(deep_link));
nyanpasu_rpc::resource!(event "service-status-changed-event", targets = [desktop, http, local], scope = sensitive_read);
nyanpasu_rpc::resource!(event "service-status-summary-changed-event", targets = [desktop, http], scope = read);
nyanpasu_rpc::resource!(event "state-changed-event", targets = [desktop, http], scope = read);
nyanpasu_rpc::resource!(event "storage-resync-event", targets = [desktop, http], scope = read);
// A storage change carries an arbitrary stored value.
nyanpasu_rpc::resource!(event "storage-value-changed-event", targets = [desktop, http], scope = sensitive_read);
// The sync status carries blocked item paths, which are local names; sync is a device-local
// capability that a browser must not read.
nyanpasu_rpc::resource!(event "sync-status-changed-event", targets = [desktop], scope = sensitive_read);
// An inter-window message carries an arbitrary payload and belongs to the desktop shell.
nyanpasu_rpc::resource!(event "window-message-event", targets = [desktop], scope = sensitive_read, capabilities(window));

nyanpasu_rpc::resource!(route "icon_fetch", targets = [http], scope = read);
nyanpasu_rpc::resource!(route "logs_archive", targets = [http], scope = sensitive_read);
nyanpasu_rpc::resource!(stream "connection_details", targets = [http, local], scope = read);

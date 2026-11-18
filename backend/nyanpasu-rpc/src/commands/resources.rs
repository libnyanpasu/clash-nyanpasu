//! The events, HTTP routes and streams a caller is admitted to, declared for the resource table.
//!
//! Each names its requirement and sensitive marking from the reviewed mapping, and the exposure
//! and capability it had before it became a resource. Each one moves into its namespace when the
//! namespace tree derives requirements.

nyanpasu_rpc::resource!(event "clash-ws-event", targets = [desktop, http, local], permission = traffic:read);
nyanpasu_rpc::resource!(event "core-logs-changed", targets = [desktop, http, local], permission = logs:read, sensitive);
nyanpasu_rpc::resource!(event "core-logs-summary-changed", targets = [desktop, http], permission = logs:read);

nyanpasu_rpc::resource!(route "logs_archive", targets = [http], permission = logs:read, sensitive);
nyanpasu_rpc::resource!(stream "connection_details", targets = [http, local], permission = traffic:read);

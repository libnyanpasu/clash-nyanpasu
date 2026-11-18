//! Application updates.

nyanpasu_rpc::resource!(event "app-update-summary-changed", targets = [desktop, http], capabilities(app_update));

nyanpasu_rpc::resource!(event "app-update-state-changed", targets = [desktop, http, local], capabilities(app_update), sensitive);

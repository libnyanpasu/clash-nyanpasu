//! The application windows.

nyanpasu_rpc::namespace!(expose = []);

// An inter-window message carries an arbitrary payload and belongs to the desktop shell.
nyanpasu_rpc::resource!(event "window-message-event", targets = [desktop], capabilities(window), sensitive);

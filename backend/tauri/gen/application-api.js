"use strict";
(() => {
    const host = window;
    const invoke = (command, args) => {
        const current = host.__TAURI_INTERNALS__?.invoke;
        if (!current)
            return Promise.reject(new Error("Tauri IPC is unavailable"));
        return current(command, args);
    };
    host.__NYANPASU_API__ = {
        call: (fnName, params) => invoke("plugin:application-api|call", { fnName, params }),
        subscribe: (fnName, params, channel) => invoke("plugin:application-api|subscribe", { fnName, params, channel }),
        unsubscribe: (subscriptionId) => invoke("plugin:application-api|unsubscribe", { subscriptionId }),
    };
})();

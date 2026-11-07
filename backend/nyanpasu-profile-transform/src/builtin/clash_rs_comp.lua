-- compatible with proxies strict port type
if config["proxies"] ~= nil and type(config["proxies"]) == "table" then
    for _, proxy in pairs(config["proxies"]) do
        if proxy["port"] ~= nil and type(proxy["port"]) == "string" then
            proxy["port"] = tonumber(proxy["port"]) or error("invalid port: " .. proxy["port"])
        end
    end
end


return config

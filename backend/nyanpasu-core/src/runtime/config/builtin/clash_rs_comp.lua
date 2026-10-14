-- compatible with ipv6 decrepation 
if config["ipv6"] ~= nil then
    config["ipv6"] = nil
    if config["dns"] ~= nil and config["dns"]["enabled"] == true then
        config["dns"]["ipv6"] = true
    end
end

-- compatible with allow lan decrepation
if config["allow_lan"] == true then
    config["allow_lan"] = nil
    config["bind_address"] = "0.0.0.0"
end

-- compatible with proxies strict port type
if config["proxies"] ~= nil and type(config["proxies"]) == "table" then
    for _, proxy in pairs(config["proxies"]) do
        if proxy["port"] ~= nil and type(proxy["port"]) == "string" then
            proxy["port"] = tonumber(proxy["port"]) or error("invalid port: " .. proxy["port"])
        end
    end
end


return config

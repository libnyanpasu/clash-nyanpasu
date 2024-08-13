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

return config

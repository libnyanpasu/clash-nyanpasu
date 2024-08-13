if type(config['log-level']) == 'boolean' then
    config['log-level'] = 'debug'
end

return config

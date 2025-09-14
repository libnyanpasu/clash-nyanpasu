# Configuration Switch and Proxy Group Sync Feature

This feature provides robust configuration management for the Clash Nyanpasu application, including configuration switching, proxy group synchronization, multi-core conflict detection, and comprehensive error handling.

## Features

### 1. Configuration Switch
- Switch between different Clash configuration profiles
- API call to `/configs/switch` with a 200ms delay for core application
- Automatic proxy group refresh after configuration switch
- Success/failure notifications

### 2. Proxy Group Synchronization
- Immediate fetch of latest proxy groups from the core after config switch
- State management for `currentConfigName` and `proxyGroups` list
- Comparison between core's `currentConfig` and UI's `currentConfig`
- Automatic reload core or user prompt to retry on mismatch

### 3. Multi-Core Conflict Detection
- Detection of running cores via `/running_cores` API
- Warning display in UI when multiple cores are detected
- Option to disable configuration switching or require user confirmation
- Core reload functionality to resolve conflicts

### 4. Error Handling & Logging
- Graceful API failure handling with user-friendly messages
- Comprehensive logging mechanism for troubleshooting
- Recovery attempts tracking (up to 3 attempts)
- Detailed error information in UI

### 5. Periodic Synchronization
- Automatic periodic check of config consistency between UI and Core
- Automatic refresh of proxy groups on mismatch
- Performance-optimized with 45-second sync intervals

## Implementation Details

### Core Components

#### 1. `useConfigSync` Hook
Location: `src/hooks/use-config-sync.ts`

A comprehensive React hook that manages all configuration synchronization logic:

- **State Management**: Uses Jotai atoms for persistent state
- **Core Communication**: Handles API calls to Clash core
- **Multi-Core Detection**: Monitors for conflicting core instances
- **Error Recovery**: Implements retry logic and recovery mechanisms
- **Performance Optimization**: Uses refs and debouncing to prevent excessive re-renders

#### 2. `ConfigSwitcher` Component
Location: `src/components/config-switcher.tsx`

A user-facing component that provides:

- Configuration profile selection dropdown
- Multi-core warning alerts
- Recovery status indicators
- Error display with recovery options
- Confirmation dialogs for risky operations
- Snackbar notifications for user feedback

#### 3. Configuration Store
Location: `src/store/config.ts`

Jotai atoms for persistent state management:

- `currentConfigNameAtom`: Tracks the currently active configuration
- `multiCoreWarningAtom`: Multi-core conflict detection state
- `configSyncStatusAtom`: Synchronization status and errors
- `recoveryAttemptsAtom`: Number of recovery attempts (persisted)
- `lastSuccessTimeAtom`: Timestamp of last successful operation (persisted)
- `configSwitchHistoryAtom`: History of configuration switches (persisted)
- `proxyGroupsAtom`: Cached proxy groups data
- `runningCoresAtom`: Information about running cores

#### 4. Logging Utility
Location: `src/utils/config-logger.ts`

Advanced logging system with:

- Multiple log levels (debug, info, warn, error)
- Automatic log deduplication
- Local storage persistence
- Real-time React hooks for log consumption
- Export functionality for troubleshooting

### Performance Considerations

1. **Memory Management**: All timeouts and intervals are properly cleaned up
2. **Debouncing**: Log persistence and UI updates use debouncing to prevent performance issues
3. **Memoization**: Expensive computations are memoized to prevent unnecessary re-renders
4. **Efficient Data Comparison**: Smart comparison algorithms to avoid unnecessary refreshes
5. **Lazy Loading**: Components and data are loaded only when needed

### Error Recovery Mechanisms

1. **Automatic Recovery**: Up to 3 automatic recovery attempts
2. **Manual Intervention**: User prompts for manual recovery when automatic recovery fails
3. **Core Reload**: Safe core restart functionality
4. **State Restoration**: Automatic restoration of previous state on failure

## Usage

### Basic Usage

```tsx
import { ConfigSwitcher } from '@/components/config-switcher'

const MyComponent = () => {
  return (
    <div>
      <ConfigSwitcher />
    </div>
  )
}
```

### Advanced Usage with Hooks

```tsx
import { useConfigSync } from '@/hooks/use-config-sync'

const MyAdvancedComponent = () => {
  const { 
    isSwitching, 
    switchError, 
    switchConfig,
    reloadCore,
    currentConfigName
  } = useConfigSync()

  const handleSwitch = async (configName: string) => {
    try {
      await switchConfig(configName)
      console.log('Switch successful!')
    } catch (error) {
      console.error('Switch failed:', error)
    }
  }

  return (
    <div>
      <button 
        onClick={() => handleSwitch('my-config')}
        disabled={isSwitching}
      >
        {isSwitching ? 'Switching...' : 'Switch Config'}
      </button>
      
      {switchError && (
        <div className="error">
          Error: {switchError}
          <button onClick={reloadCore}>Reload Core</button>
        </div>
      )}
    </div>
  )
}
```

### Accessing Logs

```tsx
import { useConfigSwitchLogs } from '@/utils/config-logger'

const LogViewer = () => {
  const logs = useConfigSwitchLogs()
  
  return (
    <div>
      {logs.map((log, index) => (
        <div key={index} className={`log-${log.level}`}>
          [{new Date(log.timestamp).toLocaleString()}] {log.level}: {log.message}
        </div>
      ))}
    </div>
  )
}
```

## API Integration

The feature integrates with the following Clash core APIs:

### Configuration Switch
```
POST /configs/switch
{
  "name": "config-name"
}
```

### Proxy Groups
```
GET /proxy_groups
```

### Running Cores
```
GET /running_cores
```

## Troubleshooting

### Common Issues

1. **Multi-Core Conflicts**: 
   - Solution: Use the "Reload Core" button to restart the Clash engine

2. **Configuration Switch Failures**:
   - Solution: Check network connectivity and try recovery options

3. **Proxy Group Sync Issues**:
   - Solution: Wait for periodic sync or manually trigger sync

### Log Analysis

Use the logging utility to export and analyze logs:

```ts
import { configSwitchLogger } from '@/utils/config-logger'

// Export logs as JSON for detailed analysis
const jsonLogs = configSwitchLogger.exportLogs('json')

// Export logs as formatted string for user viewing
const stringLogs = configSwitchLogger.exportLogs('string')
```

## Development

### Debug Mode

Enable detailed logging by setting `DEBUG_MODE` to `true` in the hook:

```ts
const DEBUG_MODE = process.env.NODE_ENV === 'development'
```

### Testing

The hook includes comprehensive error handling and recovery mechanisms. Test by:

1. Simulating network failures
2. Creating multi-core conflicts
3. Testing recovery scenarios
4. Verifying state persistence

## Future Enhancements

1. **Advanced Analytics**: Usage statistics and performance metrics
2. **Configuration Templates**: Pre-built configuration profiles
3. **Smart Recommendations**: AI-powered configuration suggestions
4. **Backup/Restore**: Configuration backup and restore functionality
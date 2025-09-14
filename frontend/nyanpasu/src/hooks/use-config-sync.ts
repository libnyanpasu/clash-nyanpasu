import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useClashAPI } from '@nyanpasu/interface/service/clash-api'
import { commands } from '@nyanpasu/interface/ipc/bindings'
import { unwrapResult } from '@nyanpasu/interface/utils'
import { useClashInfo } from '@nyanpasu/interface/ipc/use-clash-info'
import { useProfile } from '@nyanpasu/interface/ipc/use-profile'
import { useClashProxies } from '@nyanpasu/interface/ipc/use-clash-proxies'
import { CLASH_CONFIG_QUERY_KEY, CLASH_PROXIES_QUERY_KEY, RROFILES_QUERY_KEY } from '@nyanpasu/interface/ipc/consts'
import { useClashCores } from '@nyanpasu/interface/ipc/use-clash-cores'
import { useAtom } from 'jotai'
import { 
  currentConfigNameAtom, 
  multiCoreWarningAtom, 
  configSyncStatusAtom, 
  recoveryAttemptsAtom, 
  lastSuccessTimeAtom, 
  configSwitchHistoryAtom,
  proxyGroupsAtom,
  runningCoresAtom
} from '@/store'

// Debug mode flag - enable for development
const DEBUG_MODE = typeof process !== 'undefined' && process.env && process.env.NODE_ENV === 'development'

/**
 * Enhanced configuration sync hook with robust error handling, cache management, and multi-core conflict resolution
 * Optimized for performance with proper cleanup and debouncing
 */
export function useConfigSync() {
  const queryClient = useQueryClient()
  const isSyncingRef = useRef(false)
  const timeoutsRef = useRef({
    switch: null as NodeJS.Timeout | null,
    sync: null as NodeJS.Timeout | null,
    recovery: null as NodeJS.Timeout | null,
    successReset: null as NodeJS.Timeout | null
  })
  const intervalsRef = useRef({
    sync: null as NodeJS.Timeout | null,
    coreCheck: null as NodeJS.Timeout | null
  })
  const lastSuccessTimeRef = useRef(null)
  
  const [currentConfigName, setCurrentConfigName] = useAtom(currentConfigNameAtom)
  const [multiCoreWarning, setMultiCoreWarning] = useAtom(multiCoreWarningAtom)
  const [configSyncStatus, setConfigSyncStatus] = useAtom(configSyncStatusAtom)
  const [recoveryAttempts, setRecoveryAttempts] = useAtom(recoveryAttemptsAtom)
  const [lastSuccessTime, setLastSuccessTime] = useAtom(lastSuccessTimeAtom)
  const [configSwitchHistory, setConfigSwitchHistory] = useAtom(configSwitchHistoryAtom)
  const [proxyGroups, setProxyGroups] = useAtom(proxyGroupsAtom)
  const [runningCores, setRunningCores] = useAtom(runningCoresAtom)
  
  const [isRecovering, setIsRecovering] = useState(false)
  
  // Derived state
  const isSwitching = configSyncStatus.isSyncing
  const switchError = configSyncStatus.error

  // Debug logging function
  const debugLog = useCallback((message: string, ...args: any[]) => {
    if (DEBUG_MODE) {
      console.log(`[ConfigSync] ${message}`, ...args)
    }
  }, [])

  // Cleanup function for all timeouts and intervals
  const cleanup = useCallback(() => {
    // Clear all timeouts
    Object.values(timeoutsRef.current).forEach(timeout => {
      if (timeout) clearTimeout(timeout)
    })
    
    // Clear all intervals
    Object.values(intervalsRef.current).forEach(interval => {
      if (interval) clearInterval(interval)
    })
  }, [])

  // Get core data
  const { configs, proxies } = useClashAPI()
  const { data: clashInfo } = useClashInfo()
  const { query: profileQuery } = useProfile()
  const { data: proxyData, refetch: refetchProxies } = useClashProxies()
  const { query: coresQuery } = useClashCores()

  // Fetch profile function
  const fetchProfile = useCallback(async () => {
    debugLog('Fetching profile data')
    const result = unwrapResult(await commands.getProfiles())
    return result
  }, [debugLog])

  // Fetch clash info function
  const fetchClashInfo = useCallback(async () => {
    debugLog('Fetching clash info')
    const result = unwrapResult(await commands.getClashInfo())
    return result
  }, [debugLog])

  // Fetch proxies function
  const fetchProxies = useCallback(async () => {
    debugLog('Fetching proxies data')
    const result = unwrapResult(await commands.getProxies())
    return result
  }, [debugLog])

  // Queries with zero stale time to ensure fresh data
  const profileQueryData = useQuery({
    queryKey: [RROFILES_QUERY_KEY],
    queryFn: fetchProfile,
    staleTime: 0,
  })

  const clashInfoQuery = useQuery({
    queryKey: ['clash-info-fetch'],
    queryFn: fetchClashInfo,
    staleTime: 0,
  })

  const proxiesQuery = useQuery({
    queryKey: ['proxies-fetch'],
    queryFn: fetchProxies,
    staleTime: 0,
  })

  // Force refresh all related caches with error handling
  const forceRefreshAll = useCallback(async () => {
    debugLog('Force refreshing all data...')
    
    try {
      // Remove old cache entries
      queryClient.removeQueries({ queryKey: [CLASH_PROXIES_QUERY_KEY] })
      queryClient.removeQueries({ queryKey: ['proxy-groups'] })
      queryClient.removeQueries({ queryKey: ['proxy-providers'] })
      
      // Parallel refresh of all data
      const refreshPromises = [
        queryClient.refetchQueries({ queryKey: [RROFILES_QUERY_KEY] }),
        queryClient.refetchQueries({ queryKey: [CLASH_PROXIES_QUERY_KEY] }),
        queryClient.refetchQueries({ queryKey: ['clash-info-fetch'] }),
        queryClient.refetchQueries({ queryKey: [CLASH_CONFIG_QUERY_KEY] })
      ]
      
      const results = await Promise.allSettled(refreshPromises)
      
      // Check for failed refreshes
      const failures = results.filter(result => result.status === 'rejected')
      if (failures.length > 0) {
        console.warn('[ConfigSync] Some data refresh failed:', failures)
      }
      
      debugLog('Force refresh completed')
    } catch (error) {
      console.error('[ConfigSync] Force refresh error:', error)
      throw error
    }
  }, [queryClient, debugLog])

  // Wait for core to respond
  const waitForCore = useCallback(async (maxWait = 10000) => {
    const startTime = Date.now()
    
    while (Date.now() - startTime < maxWait) {
      try {
        // Try to fetch version to check if core is responsive
        const version = await fetch(`http://${clashInfo?.server || '127.0.0.1:9090'}/version`, {
          method: 'GET',
          headers: {
            ...(clashInfo?.secret ? { Authorization: `Bearer ${clashInfo.secret}` } : {})
          }
        })
        
        if (version.ok) {
          debugLog('Core is responsive')
          return true
        }
      } catch (error) {
        debugLog('Waiting for core to respond...')
      }
      
      await new Promise(resolve => setTimeout(resolve, 500))
    }
    
    throw new Error('Core did not respond within timeout')
  }, [clashInfo, debugLog])

  // Check for multiple running cores
  const checkRunningCores = useCallback(async () => {
    try {
      debugLog('Checking running cores')
      // Get core status from the backend
      const coreStatus = unwrapResult(await commands.getCoreStatus())
      const [coreInfos, , runType] = coreStatus
      
      // Update running cores atom
      if (Array.isArray(coreInfos)) {
        setRunningCores(coreInfos)
      }
      
      // Check if there are multiple cores running
      if (coreInfos && typeof coreInfos === 'object' && 'Stopped' in coreInfos) {
        // Single core case
        setMultiCoreWarning(false)
      } else if (Array.isArray(coreInfos)) {
        // Multiple cores case
        setMultiCoreWarning(Boolean(coresQuery.data && Object.keys(coresQuery.data).length > 1))
      } else {
        setMultiCoreWarning(false)
      }
    } catch (error) {
      console.error('[ConfigSync] Failed to check running cores:', error)
      setMultiCoreWarning(false)
    }
  }, [coresQuery.data, debugLog, setRunningCores])

  // Enhanced configuration switch mutation with multi-core conflict resolution
  const switchConfig = useMutation({
    mutationFn: async (configName) => {
      debugLog(`Starting config switch to: ${configName}`)
      setConfigSyncStatus(prev => ({ ...prev, isSyncing: true, error: null }))

      try {
        // 1. First check for multi-core conflicts
        await checkRunningCores()
        
        if (multiCoreWarning) {
          debugLog('Multiple cores detected, attempting to resolve...')
          
          // Try to stop other core instances
          try {
            await commands.restartSidecar()
            await new Promise(resolve => setTimeout(resolve, 2000))
            setMultiCoreWarning(false)
          } catch (error) {
            console.warn('[ConfigSync] Failed to stop other cores:', error)
          }
        }

        // 2. Execute config switch
        const response = await fetch(`http://${clashInfo?.server || '127.0.0.1:9090'}/configs/switch`, {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            ...(clashInfo?.secret ? { Authorization: `Bearer ${clashInfo.secret}` } : {})
          },
          body: JSON.stringify({ name: configName })
        })

        if (!response.ok) {
          throw new Error(`Switch failed: ${response.statusText}`)
        }

        // 3. Wait for core to reload
        debugLog('Waiting for core reload...')
        await new Promise(resolve => setTimeout(resolve, 3000))
        
        // 4. Confirm core response and check for multi-core conflicts
        await waitForCore()
        await checkRunningCores()

        // 5. Immediately force refresh all data
        await forceRefreshAll()

        // 6. Set current config name
        setCurrentConfigName(configName || null)
        // Handle potential Promise from atom
        setLastSuccessTime(Date.now())
        
        // 7. Update switch history
        const currentHistory = Array.isArray(configSwitchHistory) ? configSwitchHistory : [];
        setConfigSwitchHistory([
          ...currentHistory.slice(-9), // Keep only last 10 entries
          {
            configName: configName || '',
            timestamp: Date.now(),
            success: true
          }
        ])

        debugLog('Config switch completed successfully')
        return { success: true, config: configName }

      } catch (error: any) {
        console.error('[ConfigSync] Config switch failed:', error)
        setConfigSyncStatus(prev => ({ ...prev, error: error.message }))
        
        // Update switch history with failure
        const currentHistory = Array.isArray(configSwitchHistory) ? configSwitchHistory : [];
        setConfigSwitchHistory([
          ...currentHistory.slice(-9), // Keep only last 10 entries
          {
            configName: configName || '',
            timestamp: Date.now(),
            success: false
          }
        ])
        
        // Check multi-core status after switch error
        try {
          await checkRunningCores()
        } catch (checkError: any) {
          console.warn('[ConfigSync] Failed to check cores after switch error:', checkError)
        }
        
        throw error
      } finally {
        setConfigSyncStatus(prev => ({ ...prev, isSyncing: false }))
      }
    },
    onSuccess: (data) => {
      debugLog('Switch mutation succeeded:', data)
      // Additional post-success processing
      if (timeoutsRef.current.sync) {
        clearTimeout(timeoutsRef.current.sync)
      }
      timeoutsRef.current.sync = setTimeout(async () => {
        await forceRefreshAll()
        await checkRunningCores()
      }, 1000)
    },
    onError: (error: any) => {
      console.error('[ConfigSync] Switch mutation failed:', error)
      setConfigSyncStatus(prev => ({ ...prev, error: error.message }))
    }
  })

  // Core reload function
  const reloadCore = useCallback(async () => {
    try {
      setConfigSyncStatus(prev => ({ ...prev, isSyncing: true, error: null }))
      debugLog('Reloading core...')
      
      await commands.restartSidecar()
      
      // Wait for core restart
      await new Promise(resolve => setTimeout(resolve, 5000))
      await waitForCore()
      
      // Refresh all data
      await forceRefreshAll()
      
      // Handle potential Promise from atom
      setLastSuccessTime(Date.now())
    } catch (error: any) {
      console.error('[ConfigSync] Core reload error:', error)
      setConfigSyncStatus(prev => ({ ...prev, error: error.message }))
    } finally {
      setConfigSyncStatus(prev => ({ ...prev, isSyncing: false }))
    }
  }, [waitForCore, forceRefreshAll, debugLog])

  // Intelligent recovery function
  const recoverFromError = useCallback(async () => {
    debugLog('Attempting to recover from error state...')
    
    // Prevent multiple concurrent recovery attempts
    if (isRecovering) {
      debugLog('Recovery already in progress, skipping')
      return
    }
    
    setIsRecovering(true)
    
    try {
      // Limit recovery attempts to prevent infinite loops
      if (recoveryAttempts >= 3) {
        console.error('[ConfigSync] Too many recovery attempts, giving up')
        setConfigSyncStatus(prev => ({ ...prev, error: 'Recovery failed after multiple attempts. Please restart the application.' }))
        return
      }
      
      const currentAttempts = typeof recoveryAttempts === 'number' ? recoveryAttempts : 0;
      setRecoveryAttempts(currentAttempts + 1)
      
      // 1. Check and resolve multi-core conflicts
      await checkRunningCores()
      
      // 2. Try to reload core if multi-core warning exists
      if (multiCoreWarning) {
        await reloadCore()
      }
      
      // 3. Force refresh all data
      await forceRefreshAll()
      
      // 4. Clear error state
      setConfigSyncStatus(prev => ({ ...prev, error: null }))
      
      debugLog('Recovery completed')
    } catch (error: any) {
      console.error('[ConfigSync] Recovery failed:', error)
      setConfigSyncStatus(prev => ({ ...prev, error: `Recovery failed: ${error.message}` }))
    } finally {
      setIsRecovering(false)
    }
  }, [checkRunningCores, multiCoreWarning, reloadCore, forceRefreshAll, recoveryAttempts, isRecovering, debugLog])

  // Optimized config state sync function with multi-core checking
  const syncConfigState = useCallback(async () => {
    if (isSyncingRef.current || isSwitching) {
      return
    }

    isSyncingRef.current = true

    try {
      // Regularly check multi-core status
      await checkRunningCores()
      
      // Get latest config info
      const [profileRes, proxiesRes] = await Promise.allSettled([
        fetch('/api/profiles'),
        fetch(`http://${clashInfo?.server || '127.0.0.1:9090'}/proxies`, {
          headers: {
            ...(clashInfo?.secret ? { Authorization: `Bearer ${clashInfo.secret}` } : {})
          }
        })
      ])

      let needsRefresh = false
      let hasDataIssues = false

      // Check profile changes
      if (profileRes.status === 'fulfilled' && profileRes.value.ok) {
        const newProfile = await profileRes.value.json()
        const currentProfile = queryClient.getQueryData([RROFILES_QUERY_KEY])
        
        if (!currentProfile || (currentProfile as any).current !== (newProfile as any).current) {
          debugLog('Profile change detected')
          queryClient.setQueryData([RROFILES_QUERY_KEY], newProfile)
          setCurrentConfigName(newProfile.current || null)
          needsRefresh = true
        }
      } else {
        hasDataIssues = true
        console.warn('[ConfigSync] Failed to fetch profile data')
      }

      // Check proxy changes with smarter comparison
      if (proxiesRes.status === 'fulfilled' && proxiesRes.value.ok) {
        const newProxies = await proxiesRes.value.json()
        const currentProxies = queryClient.getQueryData([CLASH_PROXIES_QUERY_KEY])
        
        // Smarter comparison to avoid unnecessary refreshes
        const proxiesChanged = !currentProxies || 
          Object.keys((currentProxies as any).proxies || {}).length !== Object.keys((newProxies as any).proxies || {}).length ||
          JSON.stringify(Object.keys((currentProxies as any).proxies || {})) !== JSON.stringify(Object.keys((newProxies as any).proxies || {}))
        
        if (proxiesChanged) {
          debugLog('Proxies change detected')
          queryClient.setQueryData([CLASH_PROXIES_QUERY_KEY], newProxies)
          needsRefresh = true
        }
      } else {
        hasDataIssues = true
        console.warn('[ConfigSync] Failed to fetch proxies data')
      }

      // Trigger refresh if needed or if there are data issues
      if (needsRefresh || hasDataIssues) {
        debugLog('Triggering refresh due to', needsRefresh ? 'changes' : 'data issues')
        if (timeoutsRef.current.sync) {
          clearTimeout(timeoutsRef.current.sync)
        }
        timeoutsRef.current.sync = setTimeout(() => {
          queryClient.invalidateQueries({ queryKey: [CLASH_PROXIES_QUERY_KEY] })
          queryClient.invalidateQueries({ queryKey: ['proxy-groups'] })
        }, 500)
      }

    } catch (error: any) {
      console.error('[ConfigSync] Error during config sync:', error)
      
      // If sync error occurs, it might be due to multi-core conflicts
      try {
        await checkRunningCores()
      } catch (checkError: any) {
        console.warn('[ConfigSync] Failed to check cores after sync error:', checkError)
      }
    } finally {
      isSyncingRef.current = false
    }
  }, [isSwitching, queryClient, checkRunningCores, clashInfo, debugLog])

  // Set up sync interval
  useEffect(() => {
    if (intervalsRef.current.sync) {
      clearInterval(intervalsRef.current.sync)
    }
    
    // Reduce sync frequency to 45 seconds to reduce server load
    intervalsRef.current.sync = setInterval(syncConfigState, 45000)
    
    return () => {
      if (intervalsRef.current.sync) {
        clearInterval(intervalsRef.current.sync)
      }
    }
  }, [syncConfigState])

  // Sync when page becomes visible
  useEffect(() => {
    const handleVisibilityChange = () => {
      if (document.visibilityState === 'visible') {
        debugLog('Page became visible, syncing...')
        // Delay execution to avoid frequent calls
        if (timeoutsRef.current.switch) {
          clearTimeout(timeoutsRef.current.switch)
        }
        timeoutsRef.current.switch = setTimeout(syncConfigState, 1000)
      }
    }

    document.addEventListener('visibilitychange', handleVisibilityChange)
    return () => {
      document.removeEventListener('visibilitychange', handleVisibilityChange)
    }
  }, [syncConfigState, debugLog])

  // Initialize and periodically check for multiple cores
  useEffect(() => {
    checkRunningCores()
    intervalsRef.current.coreCheck = setInterval(checkRunningCores, 10000) // Check every 10 seconds
    return () => {
      if (intervalsRef.current.coreCheck) {
        clearInterval(intervalsRef.current.coreCheck)
      }
    }
  }, [checkRunningCores])

  // Reset recovery attempts after successful operation
  useEffect(() => {
    if (lastSuccessTimeRef.current && recoveryAttempts > 0) {
      if (timeoutsRef.current.successReset) {
        clearTimeout(timeoutsRef.current.successReset)
      }
      timeoutsRef.current.successReset = setTimeout(() => {
        setRecoveryAttempts(0)
        debugLog('Resetting recovery attempts after successful operation')
      }, 30000) // Reset after 30 seconds of success
    }
  }, [recoveryAttempts, debugLog])

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      cleanup()
    }
  }, [cleanup])

  // Memoize the return value to prevent unnecessary re-renders
  return {
    // State (from store)
    isSwitching,
    switchError,
    multiCoreWarning,
    currentConfigName,
    recoveryAttempts,
    isRecovering,
    lastSuccessTime,
    configSwitchHistory,
    proxyGroups,
    runningCores,

    // Functions
    switchConfig: switchConfig.mutateAsync,
    reloadCore,
    syncConfigState,
    forceRefreshAll,
    recoverFromError,
    checkRunningCores,

    // Data
    clashInfo: clashInfoQuery.data,
    profileData: profileQueryData.data,
    proxyData: proxiesQuery.data
  }
}
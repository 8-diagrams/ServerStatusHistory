<template>
  <ServerDetailPage
    v-if="detailRoute"
    :key="`${detailRoute.name}\u0000${detailRoute.gid || ''}`"
    :host="detailRoute.name"
    :gid="detailRoute.gid"
  />
  <App v-else />
</template>

<script setup lang="ts">
import App from '@/App.vue'
import ServerDetailPage from '@/components/ServerDetailPage.vue'
import { parseServerDetailRoute } from '@/routes'

const currentHash = ref(typeof window === 'undefined' ? '' : window.location.hash)
const detailRoute = computed(() => parseServerDetailRoute(currentHash.value))

function syncRoute() {
  currentHash.value = window.location.hash
}

onMounted(() => window.addEventListener('hashchange', syncRoute))
onUnmounted(() => window.removeEventListener('hashchange', syncRoute))
</script>

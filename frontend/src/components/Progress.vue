<template>
  <div
    class="relative inline-block overflow-hidden rounded-full bg-[#eeeaf7] text-sm text-[#3f3850] shadow-[inset_0_1px_2px_rgba(91,53,200,0.10)]"
  >
    <div
      class="whitespace-nowrap px-2 text-sm shadow-[inset_0_1px_0_rgba(255,255,255,0.28)] transition-[width,background-color] duration-500"
      :class="{
        'bg-[#b8a9ed]': ratio < 0.8,
        'bg-[#d7a0d8]': ratio >= 0.8 && ratio < 0.9,
        'bg-[#e893a4]': ratio >= 0.9,
      }"
      :style="{
        width: `${ratio * 100}%`,
      }"
    >
      <slot />
    </div>
  </div>
</template>

<script setup lang="ts">
const props = defineProps<{
  value: number
  max: number
}>()

const ratio = computed(() => props.max > 0 ? Math.min(1, Math.max(0, props.value / props.max)) : 0)
</script>

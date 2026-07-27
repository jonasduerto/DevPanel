<script>
  let { icon = "", title, description = "", children } = $props();
</script>

<div class="card-header">
  {#if icon}<span class="card-icon">{icon}</span>{/if}
  <div class="copy"><div class="card-title">{title}</div>{#if description}<div class="card-desc">{description}</div>{/if}</div>
  {@render children?.()}
</div>


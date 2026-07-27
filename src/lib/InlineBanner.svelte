<script>
  let { variant = "error", children } = $props();
</script>

<div class="banner {variant}" role={variant === "error" ? "alert" : "status"}>{@render children?.()}</div>


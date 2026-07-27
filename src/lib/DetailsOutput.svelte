<script>
  let { title = "Output", value = "", open = true } = $props();
</script>

<details class="details-output" {open}>
  <summary>{title}</summary>
  <pre>{value}</pre>
</details>


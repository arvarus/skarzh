<script lang="ts">
  import { onMount } from "svelte";
  import { fetchAppInfo, formatAppInfo } from "$lib/app-info";

  let appInfo = $state("");
  let error = $state("");

  onMount(async () => {
    try {
      appInfo = formatAppInfo(await fetchAppInfo());
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  });
</script>

<main class="container">
  <h1>skarzh</h1>
  {#if error}
    <p class="error">Could not reach the backend: {error}</p>
  {:else}
    <p>{appInfo}</p>
  {/if}
</main>

<style>
  :root {
    font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
    font-size: 16px;
    line-height: 24px;
    font-weight: 400;
    color: #0f0f0f;
    background-color: #f6f6f6;
    font-synthesis: none;
    text-rendering: optimizeLegibility;
  }

  .container {
    padding-top: 10vh;
    text-align: center;
  }

  .error {
    color: #b00020;
  }

  @media (prefers-color-scheme: dark) {
    :root {
      color: #f6f6f6;
      background-color: #2f2f2f;
    }

    .error {
      color: #ff6b81;
    }
  }
</style>

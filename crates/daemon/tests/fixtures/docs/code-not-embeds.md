A `chart` fence and ```svg``` in inline code are not embeds, and neither is one inside another fence:

~~~text
```chart
not json
```
```svg
<p>not a drawing</p>
```
~~~

```json
{"type": "pie"}
```

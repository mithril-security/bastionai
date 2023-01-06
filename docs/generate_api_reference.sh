pip install pdoc3 markdownify
rm -rf docs/resources
pdoc --skip-errors --template-dir docs/pdoc_template -o docs/docs/resources client/src/bastionlab
sed -i '/<p>Generated by <a href="https:\/\/pdoc3.github.io\/pdoc" title="pdoc: Python API documentation generator"><cite>pdoc<\/cite> 0.10.0<\/a>.<\/p>/d' docs/docs/resources/bastionlab/*.md docs/docs/resources/bastionlab/*/*.md

# Fix broken links
sed -i 's/bastionlab.client/[bastionlab.client](client.md)/g' docs/docs/resources/bastionlab/index.md
sed -i 's/bastionlab.errors/[bastionlab.errors](errors.md)/g' docs/docs/resources/bastionlab/index.md
sed -i 's/bastionlab.keys/[bastionlab.keys](keys.md)/g' docs/docs/resources/bastionlab/index.md
sed -i 's+bastionlab.pb+[bastionlab.pb](pb/index.md)+g' docs/docs/resources/bastionlab/index.md
sed -i 's+bastionlab.polars+[bastionlab.polars](polars/index.md)+g' docs/docs/resources/bastionlab/index.md
sed -i 's+bastionlab.torch+[bastionlab.torch](torch/index.md)+g' docs/docs/resources/bastionlab/index.md
sed -i 's+* bastionlab.version++g' docs/docs/resources/bastionlab/index.md

sed -i 's+bastionlab.polars.client+[bastionlab.polars.client](client.md)+g' docs/docs/resources/bastionlab/polars/index.md
sed -i 's+bastionlab.polars.policy+[bastionlab.polars.policy](policy.md)+g' docs/docs/resources/bastionlab/polars/index.md
sed -i 's+bastionlab.polars.remote_polars+[bastionlab.polars.remote_polars](remote_polars.md)+g' docs/docs/resources/bastionlab/polars/index.md
sed -i 's+bastionlab.polars.utils+[bastionlab.polars.utils](utils.md)+g' docs/docs/resources/bastionlab/polars/index.md

sed -i 's+bastionlab.torch.client+[bastionlab.torch.client](client.md)+g' docs/docs/resources/bastionlab/torch/index.md
sed -i 's+bastionlab.torch.learner+[bastionlab.torch.learner](learner.md)+g' docs/docs/resources/bastionlab/torch/index.md
sed -i 's+bastionlab.torch.optimizer_config+[bastionlab.torch.optimizer_config](optimizer_config.md)+g' docs/docs/resources/bastionlab/torch/index.md
sed -i 's+bastionlab.torch.utils+[bastionlab.torch.utils](utils.md)+g' docs/docs/resources/bastionlab/torch/index.md
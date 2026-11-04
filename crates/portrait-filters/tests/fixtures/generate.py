#!/usr/bin/env python3
"""Regenerate the synthetic ONNX graphs in this directory.

These graphs are host tests (identity, multiply by 0.5, fixed 4x4 identity).
They are not portrait weights and they are not part of a training pipeline.
Requires the `onnx` Python package.
"""

from pathlib import Path

import numpy as np
import onnx
from onnx import TensorProto, helper, numpy_helper

OUT = Path(__file__).resolve().parent


def save(model, name: str) -> None:
    model.ir_version = 8
    model.producer_name = "photocraft-portrait-filters"
    model.producer_version = "test-fixture"
    model.doc_string = "Synthetic ONNX graph for portrait-filter host tests. Not a trained retouch weight."
    onnx.checker.check_model(model)
    onnx.save(model, OUT / name)


def make(nodes, inputs, outputs, initializers, name: str):
    graph = helper.make_graph(nodes, name, inputs, outputs, initializers)
    return helper.make_model(graph, opset_imports=[helper.make_opsetid("", 17)])


def main() -> None:
    dynamic_in = helper.make_tensor_value_info("image", TensorProto.FLOAT, ["n", 3, "h", "w"])
    dynamic_out = helper.make_tensor_value_info("image_out", TensorProto.FLOAT, ["n", 3, "h", "w"])
    save(make([helper.make_node("Identity", ["image"], ["image_out"])], [dynamic_in], [dynamic_out], [], "identity"), "identity.onnx")
    half = numpy_helper.from_array(np.array(0.5, dtype=np.float32), name="half")
    save(
        make([helper.make_node("Mul", ["image", "half"], ["image_out"])], [dynamic_in], [dynamic_out], [half], "half"),
        "half.onnx",
    )
    fixed_in = helper.make_tensor_value_info("image", TensorProto.FLOAT, [1, 3, 4, 4])
    fixed_out = helper.make_tensor_value_info("image_out", TensorProto.FLOAT, [1, 3, 4, 4])
    save(make([helper.make_node("Identity", ["image"], ["image_out"])], [fixed_in], [fixed_out], [], "fixed4"), "fixed4.onnx")


if __name__ == "__main__":
    main()

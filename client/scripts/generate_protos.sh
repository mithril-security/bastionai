#!/bin/bash

PROJ_DIR=$(realpath $(dirname ${BASH_SOURCE[0]}))

PROTO_DIR=${PROJ_DIR}/../bastionai/pb

mkdir -p ${PROTO_DIR}
echo $PROJ_DIR
python -m grpc_tools.protoc -I${PROJ_DIR}/../bastionai/protos \
                            --python_out=${PROTO_DIR} \
                            --grpc_python_out=${PROTO_DIR} \
                            ${PROJ_DIR}/../bastionai/protos/remote_torch.proto